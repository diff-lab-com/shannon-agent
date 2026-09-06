//! P1-6 — Migration wizard backend: scan / preview / apply.
//!
//! Imports five asset classes from an existing Claude Code or ZCode install
//! into Shannon's existing stores:
//!
//! | kind             | source (claude-code)              | source (zcode)                    | Shannon destination                     |
//! |------------------|-----------------------------------|-----------------------------------|-----------------------------------------|
//! | `mcp`            | `.mcp.json`, `~/.claude.json`, `~/.claude/settings.json` | `~/.zcode/settings.json` (`mcpServers`) | `~/.shannon/desktop/mcp-servers.json`   |
//! | `skill`          | `~/.claude/skills/*/SKILL.md`     | `~/.zcode/skills/*/SKILL.md`      | `~/.shannon/skills/<name>/`             |
//! | `command`        | `~/.claude/commands/**/*.md`      | `~/.zcode/commands/**/*.md`       | `~/.shannon/commands/<rel>.md`          |
//! | `memory`         | `<project>/CLAUDE.md`             | `<project>/AGENTS.md` + `~/.zcode/AGENTS.md` | `MemoryStore` (`~/.shannon/memories/`) |
//! | `settings-rules` | `~/.claude/settings.json` `permissions.allow` | `~/.zcode/settings.json` `permissions.allow` | `.shannon/profiles/<source>-imported.toml` |
//!
//! Frozen wire contract (P1-6):
//! - `migration_scan({ source }) -> MigrationScanResult`
//! - `migration_preview({ source, items }) -> MigrationPreviewResult`
//! - `migration_apply({ source, items }) -> MigrationApplyReport`
//!   where an item is `{ id, action: 'import'|'skip' }` plus an **optional,
//!   additive** `conflict: 'overwrite'|'rename'|'skip'` hint used only when
//!   the target already exists with different content (default: `rename`,
//!   i.e. write `<name>-imported`).
//!
//! Safety rules (brief):
//! - only the fixed, well-known source paths above are ever read — the
//!   commands accept **no** user-supplied paths, and every discovered source
//!   path must canonicalize under one of the whitelisted roots
//!   (`~/.claude`, `~/.zcode`, project dir) or it is dropped;
//! - nothing from the source side is ever executed — files are read and
//!   copied verbatim at most;
//! - a source dir that does not exist yields an empty result (the missing
//!   slots are listed in `notFound`), never an error; one corrupted source
//!   file fails only that slot (recorded in `errors`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use shannon_core::memory::{MemoryCategory, MemoryEntry, MemoryStore};

use crate::config::McpServerConfig;

// ─── Source whitelist ───────────────────────────────────────────────────────

pub const SOURCE_CLAUDE_CODE: &str = "claude-code";
pub const SOURCE_ZCODE: &str = "zcode";

/// The two supported migration sources. Anything else is rejected — the
/// source argument is a whitelist lookup, never a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationSource {
    ClaudeCode,
    ZCode,
}

impl MigrationSource {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            SOURCE_CLAUDE_CODE => Ok(Self::ClaudeCode),
            SOURCE_ZCODE => Ok(Self::ZCode),
            other => Err(format!(
                "unsupported migration source {other:?} — expected \
                 {SOURCE_CLAUDE_CODE:?} or {SOURCE_ZCODE:?}"
            )),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => SOURCE_CLAUDE_CODE,
            Self::ZCode => SOURCE_ZCODE,
        }
    }

    /// Home-relative probe directory: `~/.claude` or `~/.zcode`.
    fn home_dir_name(&self) -> &'static str {
        match self {
            Self::ClaudeCode => ".claude",
            Self::ZCode => ".zcode",
        }
    }

    /// Project-level instruction file this source uses.
    fn project_memory_file(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "CLAUDE.md",
            Self::ZCode => "AGENTS.md",
        }
    }
}

/// Filesystem roots every scan/preview/apply is anchored to. Production
/// resolves these from the environment once per invocation; tests inject
/// tempdirs so no test ever touches the real `~`.
#[derive(Debug, Clone)]
pub struct Roots {
    pub home: PathBuf,
    pub project: PathBuf,
}

impl Roots {
    /// Resolve from the environment (`HOME` / `USERPROFILE`, then cwd).
    fn from_env() -> Result<Self, String> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .map_err(|_| "could not resolve home directory".to_string())?;
        let project = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Ok(Self { home, project })
    }

    fn source_home(&self, source: MigrationSource) -> PathBuf {
        self.home.join(source.home_dir_name())
    }
}

// ─── DTOs (frozen wire shapes) ──────────────────────────────────────────────

/// One migratable asset. Field set is frozen (`camelCase` on the wire).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationAsset {
    /// Stable id: `<source>:<kind>:<slug>` — deterministic across scans of
    /// an unchanged source tree, so preview/apply can resolve it.
    pub id: String,
    /// `mcp` | `skill` | `command` | `memory` | `settings-rules`
    pub kind: String,
    pub name: String,
    pub source_path: String,
    pub target_path: String,
    /// `none` (target free) | `overwrite` (target exists, differs) |
    /// `skip-existing` (target exists with identical content)
    pub conflict: String,
    /// Approximate source size in bytes (files: len; skill dirs: sum).
    pub size_hint: u64,
}

/// A non-fatal scan problem: one corrupted / unsupported source file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationScanError {
    pub path: String,
    pub error: String,
}

/// Result of `migration_scan`. `items` holds only what was actually found;
/// `notFound` names the well-known slots that were probed but are absent
/// (「未发现」) so the UI can show what was checked.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationScanResult {
    pub source: String,
    pub items: Vec<MigrationAsset>,
    pub not_found: Vec<String>,
    pub errors: Vec<MigrationScanError>,
}

impl MigrationScanResult {
    fn empty(source: MigrationSource) -> Self {
        Self {
            source: source.as_str().to_string(),
            items: Vec::new(),
            not_found: Vec::new(),
            errors: Vec::new(),
        }
    }
}

/// One requested item. `action` is frozen as `import` | `skip`.
/// `conflict` is an **additive, optional** hint (P1-6): how to treat an
/// existing, differing target — `overwrite` | `rename` (default) | `skip`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MigrationItemInput {
    pub id: String,
    pub action: String,
    #[serde(default)]
    pub conflict: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreviewItem {
    pub id: String,
    pub diff_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreviewResult {
    pub per_item: Vec<MigrationPreviewItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationApplyFailure {
    pub id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationApplyReport {
    pub imported: usize,
    pub skipped: usize,
    pub failed: Vec<MigrationApplyFailure>,
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

/// Scan a source install for migratable assets. Missing home / missing
/// source dirs are an **empty result**, never an error.
#[tauri::command]
pub async fn migration_scan(source: String) -> Result<MigrationScanResult, String> {
    let source = MigrationSource::parse(&source)?;
    let roots = Roots::from_env()?;
    Ok(scan_core(source, &roots))
}

/// Per-item conflict preview for the requested ids (re-scans the source so
/// the summary reflects current on-disk state).
#[tauri::command]
pub async fn migration_preview(
    source: String,
    items: Vec<MigrationItemInput>,
) -> Result<MigrationPreviewResult, String> {
    let source = MigrationSource::parse(&source)?;
    let roots = Roots::from_env()?;
    Ok(preview_core(source, &items, &roots))
}

/// Execute the approved imports. Read-scan + copy/merge only — nothing from
/// the source side is executed. Idempotent: re-importing an unchanged source
/// surfaces as `skipped` (content-identity conflict handling), never as a
/// duplicated copy.
#[tauri::command]
pub async fn migration_apply(
    source: String,
    items: Vec<MigrationItemInput>,
) -> Result<MigrationApplyReport, String> {
    let source = MigrationSource::parse(&source)?;
    let roots = Roots::from_env()?;
    apply_core(source, &items, &roots)
}

// ─── Destination paths (Shannon stores) ─────────────────────────────────────

/// MCP servers live in Shannon's existing desktop store
/// (`~/.shannon/desktop/mcp-servers.json`, same JSON `Vec<McpServerConfig>`
/// shape `crate::config::load_mcp_servers` reads).
fn mcp_store_path(roots: &Roots) -> PathBuf {
    roots
        .home
        .join(".shannon")
        .join("desktop")
        .join("mcp-servers.json")
}

fn shannon_skills_dir(roots: &Roots) -> PathBuf {
    roots.home.join(".shannon").join("skills")
}

fn shannon_commands_dir(roots: &Roots) -> PathBuf {
    roots.home.join(".shannon").join("commands")
}

fn shannon_memories_dir(roots: &Roots) -> PathBuf {
    roots.home.join(".shannon").join("memories")
}

fn shannon_profiles_dir(roots: &Roots) -> PathBuf {
    roots.project.join(".shannon").join("profiles")
}

// ─── Scan ───────────────────────────────────────────────────────────────────

fn scan_core(source: MigrationSource, roots: &Roots) -> MigrationScanResult {
    let mut result = MigrationScanResult::empty(source);
    let src_home = roots.source_home(source);

    // MCP — project `.mcp.json` (claude-code) wins over `~/.claude.json`
    // which wins over `settings.json`. Same-name servers seen twice are
    // reported as ignored duplicates rather than silently merged.
    let mut mcp_priorities: Vec<(String, PathBuf)> = Vec::new();
    match source {
        MigrationSource::ClaudeCode => {
            mcp_priorities.push(("mcp (project)".into(), roots.project.join(".mcp.json")));
            mcp_priorities.push(("mcp (global)".into(), roots.home.join(".claude.json")));
            mcp_priorities.push(("mcp (settings)".into(), src_home.join("settings.json")));
        }
        MigrationSource::ZCode => {
            mcp_priorities.push(("mcp (settings)".into(), src_home.join("settings.json")));
        }
    }
    let mut seen_servers: Vec<String> = Vec::new();
    let mut any_mcp_file = false;
    for (_slot, path) in &mcp_priorities {
        if !path.is_file() {
            continue;
        }
        any_mcp_file = true;
        scan_mcp_file(path, &seen_servers, &mut result, roots);
        for item in &result.items {
            if item.kind == "mcp" && !seen_servers.iter().any(|n| n == &item.name) {
                seen_servers.push(item.name.clone());
            }
        }
    }
    if !any_mcp_file {
        for (slot, path) in &mcp_priorities {
            result
                .not_found
                .push(format!("{slot} — {}", path.display()));
        }
    }

    // Settings allow-rules (same settings.json as above for MCP).
    let settings_path = src_home.join("settings.json");
    if settings_path.is_file() {
        scan_settings_rules(&settings_path, source, &mut result, roots);
    } else {
        result
            .not_found
            .push(format!("settings — {}", settings_path.display()));
    }

    // Skills: `~/.<source>/skills/<name>/SKILL.md`.
    let skills_dir = src_home.join("skills");
    if skills_dir.is_dir() {
        scan_skills_dir(&skills_dir, source, &mut result, roots);
    } else {
        result
            .not_found
            .push(format!("skills — {}", skills_dir.display()));
    }

    // Commands: `~/.<source>/commands/**/*.md`.
    let commands_dir = src_home.join("commands");
    if commands_dir.is_dir() {
        scan_commands_dir(&commands_dir, source, &mut result, roots);
    } else {
        result
            .not_found
            .push(format!("commands — {}", commands_dir.display()));
    }

    // Project memory file (claude-code: CLAUDE.md; zcode: AGENTS.md).
    let project_memory = roots.project.join(source.project_memory_file());
    if project_memory.is_file() {
        push_memory_asset(&project_memory, source, roots, &mut result);
    } else {
        result
            .not_found
            .push(format!("memory (project) — {}", project_memory.display()));
    }

    // ZCode two-level AGENTS.md: global level lives at `~/.zcode/AGENTS.md`.
    if source == MigrationSource::ZCode {
        let global_memory = src_home.join("AGENTS.md");
        if global_memory.is_file() {
            push_memory_asset(&global_memory, source, roots, &mut result);
        } else {
            result
                .not_found
                .push(format!("memory (global) — {}", global_memory.display()));
        }
    }

    result
        .items
        .sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
    result
}

/// Defensive whitelist: a discovered source path is only usable when its
/// canonical form stays inside one of the **exact source roots** — the probe
/// directory (`~/.claude` or `~/.zcode`) and the project dir. Deliberately
/// *not* all of `$HOME`: a symlink like `~/.claude/skills/evil -> ~/notes`
/// must be rejected, so "somewhere in home" is never good enough. (Fixed-path
/// files such as `~/.claude.json` never traverse this guard — they are
/// constructed from known templates, not discovered by traversal.)
fn is_whitelisted_source(path: &Path, roots: &Roots, source: MigrationSource) -> bool {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    [roots.source_home(source), roots.project.clone()]
        .iter()
        .any(|root| canonical.starts_with(root.canonicalize().unwrap_or_else(|_| root.clone())))
}

/// Parse one `mcpServers` JSON file and push importable command-transport
/// servers as assets. URL-transport servers and unreadable files land in
/// `errors` — a single bad file never aborts the scan.
fn scan_mcp_file(path: &Path, seen: &[String], result: &mut MigrationScanResult, roots: &Roots) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            result.errors.push(MigrationScanError {
                path: path.display().to_string(),
                error: format!("unreadable: {e}"),
            });
            return;
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            result.errors.push(MigrationScanError {
                path: path.display().to_string(),
                error: format!("invalid JSON: {e}"),
            });
            return;
        }
    };
    let Some(servers) = value.get("mcpServers").and_then(|s| s.as_object()) else {
        return;
    };
    for (name, spec) in servers {
        if seen.iter().any(|n| n == name) {
            result.errors.push(MigrationScanError {
                path: path.display().to_string(),
                error: format!("duplicate MCP server {name:?} ignored (already found earlier)"),
            });
            continue;
        }
        let Some(command) = spec
            .get("command")
            .and_then(|c| c.as_str())
            .filter(|c| !c.is_empty())
        else {
            result.errors.push(MigrationScanError {
                path: path.display().to_string(),
                error: format!(
                    "MCP server {name:?} has no command transport (URL-based servers are not \
                     importable)"
                ),
            });
            continue;
        };
        let args: Vec<String> = spec
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let mut env = std::collections::HashMap::new();
        if let Some(map) = spec.get("env").and_then(|e| e.as_object()) {
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    env.insert(k.clone(), s.to_string());
                }
            }
        }
        let existing = load_mcp_store(roots);
        let conflict = match existing.iter().find(|s| &s.name == name) {
            None => "none",
            Some(cur) => {
                if cur.command == command && cur.args == args && cur.env == env {
                    "skip-existing"
                } else {
                    "overwrite"
                }
            }
        };
        let size_hint = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        result.items.push(MigrationAsset {
            id: asset_id(result.source.as_str(), "mcp", name),
            kind: "mcp".into(),
            name: name.clone(),
            source_path: path.display().to_string(),
            target_path: mcp_store_path(roots).display().to_string(),
            conflict: conflict.into(),
            size_hint,
        });
    }
}

/// `permissions.allow` rules from a settings.json become one `settings-rules`
/// asset that imports into `.shannon/profiles/<source>-imported.toml`.
fn scan_settings_rules(
    path: &Path,
    source: MigrationSource,
    result: &mut MigrationScanResult,
    roots: &Roots,
) {
    let rules = read_settings_allow_rules(path, result);
    if rules.is_empty() {
        return;
    }
    let profile_name = format!("{}-imported", source.as_str());
    let target = shannon_profiles_dir(roots).join(format!("{profile_name}.toml"));
    let rendered = render_profile_toml(&profile_name, &rules);
    let conflict = match std::fs::read_to_string(&target) {
        Err(_) => "none",
        Ok(existing) if existing == rendered => "skip-existing",
        Ok(_) => "overwrite",
    };
    let size_hint = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    result.items.push(MigrationAsset {
        id: asset_id(result.source.as_str(), "settings-rules", "settings.json"),
        kind: "settings-rules".into(),
        name: format!("{profile_name}.toml"),
        source_path: path.display().to_string(),
        target_path: target.display().to_string(),
        conflict: conflict.into(),
        size_hint,
    });
}

/// Read `permissions.allow` from a settings.json; a corrupted file records a
/// scan error and yields an empty rule set.
fn read_settings_allow_rules(path: &Path, result: &mut MigrationScanResult) -> Vec<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            result.errors.push(MigrationScanError {
                path: path.display().to_string(),
                error: format!("unreadable: {e}"),
            });
            return Vec::new();
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            result.errors.push(MigrationScanError {
                path: path.display().to_string(),
                error: format!("invalid JSON: {e}"),
            });
            return Vec::new();
        }
    };
    value
        .get("permissions")
        .and_then(|p| p.get("allow"))
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Scan a skills directory: one asset per `<name>/SKILL.md` subdirectory.
fn scan_skills_dir(
    dir: &Path,
    source: MigrationSource,
    result: &mut MigrationScanResult,
    roots: &Roots,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            result.errors.push(MigrationScanError {
                path: dir.display().to_string(),
                error: format!("unreadable: {e}"),
            });
            return;
        }
    };
    for entry in entries.flatten() {
        let skill_dir = entry.path();
        if !skill_dir.is_dir() {
            continue;
        }
        let skill_md = skill_dir.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        // File-granularity guard: a symlinked SKILL.md must never be opened
        // (its content feeds the conflict comparison), only reported.
        if std::fs::symlink_metadata(&skill_md)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            result.errors.push(MigrationScanError {
                path: skill_md.display().to_string(),
                error: "refusing to follow symlink — remove it from the source and re-scan".into(),
            });
            continue;
        }
        let Some(name) = skill_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_whitelisted_source(&skill_dir, roots, source) {
            result.errors.push(MigrationScanError {
                path: skill_dir.display().to_string(),
                error: "outside the whitelisted source roots — skipped".into(),
            });
            continue;
        }
        let target_dir = shannon_skills_dir(roots).join(name);
        let target_md = target_dir.join("SKILL.md");
        let conflict = content_conflict(&skill_md, &target_md);
        result.items.push(MigrationAsset {
            id: asset_id(result.source.as_str(), "skill", name),
            kind: "skill".into(),
            name: name.to_string(),
            source_path: skill_dir.display().to_string(),
            target_path: target_dir.display().to_string(),
            conflict: conflict.into(),
            size_hint: dir_size(&skill_dir),
        });
    }
}

/// Scan a commands directory: one asset per `*.md` file (recursive; the
/// relative path without extension is the command name).
fn scan_commands_dir(
    dir: &Path,
    source: MigrationSource,
    result: &mut MigrationScanResult,
    roots: &Roots,
) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(e) => e,
            Err(e) => {
                result.errors.push(MigrationScanError {
                    path: current.display().to_string(),
                    error: format!("unreadable: {e}"),
                });
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if !is_whitelisted_source(&path, roots, source) {
                result.errors.push(MigrationScanError {
                    path: path.display().to_string(),
                    error: "outside the whitelisted source roots — skipped".into(),
                });
                continue;
            }
            let rel = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let name = rel.trim_end_matches(".md").to_string();
            let target = shannon_commands_dir(roots).join(&rel);
            let conflict = content_conflict(&path, &target);
            let size_hint = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            result.items.push(MigrationAsset {
                id: asset_id(result.source.as_str(), "command", &name),
                kind: "command".into(),
                name,
                source_path: path.display().to_string(),
                target_path: target.display().to_string(),
                conflict: conflict.into(),
                size_hint,
            });
        }
    }
}

/// Register one memory-file asset (CLAUDE.md / AGENTS.md).
fn push_memory_asset(
    path: &Path,
    source: MigrationSource,
    roots: &Roots,
    result: &mut MigrationScanResult,
) {
    let is_global = !roots
        .project
        .canonicalize()
        .map(|p| {
            path.canonicalize()
                .map(|c| c.starts_with(p))
                .unwrap_or(false)
        })
        .unwrap_or(false);
    let label = if is_global {
        "AGENTS.md (global)"
    } else {
        source.project_memory_file()
    };
    // Whitelist BEFORE reading content: a symlinked memory file must never be
    // opened, only rejected.
    if !is_whitelisted_source(path, roots, source) {
        result.errors.push(MigrationScanError {
            path: path.display().to_string(),
            error: "outside the whitelisted source roots — skipped".into(),
        });
        return;
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            result.errors.push(MigrationScanError {
                path: path.display().to_string(),
                error: format!("unreadable: {e}"),
            });
            return;
        }
    };
    let project = roots.project.display().to_string();
    let already = memory_entry_exists(&content, &project, roots);
    let size_hint = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    result.items.push(MigrationAsset {
        id: asset_id(
            result.source.as_str(),
            "memory",
            if is_global {
                "agents-md-global"
            } else {
                "project-memory"
            },
        ),
        kind: "memory".into(),
        name: label.to_string(),
        source_path: path.display().to_string(),
        target_path: shannon_memories_dir(roots).display().to_string(),
        conflict: if already { "skip-existing" } else { "none" }.into(),
        size_hint,
    });
}

// ─── Preview ────────────────────────────────────────────────────────────────

fn preview_core(
    source: MigrationSource,
    items: &[MigrationItemInput],
    roots: &Roots,
) -> MigrationPreviewResult {
    let scan = scan_core(source, roots);
    let mut per_item = Vec::new();
    for requested in items {
        let summary = match scan.items.iter().find(|a| a.id == requested.id) {
            None => "Unknown item — the source no longer provides it. Re-run the scan.".to_string(),
            Some(_asset) if requested.action != "import" => {
                "Not selected — will be skipped.".to_string()
            }
            Some(asset) => diff_summary(asset, source, roots),
        };
        per_item.push(MigrationPreviewItem {
            id: requested.id.clone(),
            diff_summary: summary,
        });
    }
    MigrationPreviewResult { per_item }
}

/// Human-readable one-line diff summary for the preview step.
fn diff_summary(asset: &MigrationAsset, source: MigrationSource, roots: &Roots) -> String {
    match asset.kind.as_str() {
        "mcp" => {
            let store = load_mcp_store(roots);
            match store.iter().find(|s| s.name == asset.name) {
                None => format!(
                    "New MCP server '{}' (command: {}) will be added to Shannon's MCP config and \
                     marked unverified — first use goes through the normal permission approval.",
                    asset.name,
                    summarize_source_mcp(asset),
                ),
                Some(cur) => {
                    let incoming = summarize_source_mcp(asset);
                    let current = format!("{} {}", cur.command, cur.args.join(" "));
                    if incoming == current {
                        format!(
                            "Server '{}' already exists with an identical config — nothing to do.",
                            asset.name
                        )
                    } else {
                        format!(
                            "Server '{}' exists with a different config (existing: '{} {}'). \
                             Your conflict choice decides overwrite vs rename.",
                            asset.name,
                            cur.command,
                            cur.args.join(" "),
                        )
                    }
                }
            }
        }
        "skill" => match asset.conflict.as_str() {
            "none" => format!(
                "New skill '{}' ({} KB) — copies to {}.",
                asset.name,
                asset.size_hint / 1024,
                asset.target_path
            ),
            "skip-existing" => format!(
                "Skill '{}' already exists with identical content — nothing to do.",
                asset.name
            ),
            _ => format!(
                "Skill '{}' exists and differs ({} KB vs target) — your conflict choice decides \
                 overwrite vs rename to '{}-imported'.",
                asset.name,
                asset.size_hint / 1024,
                asset.name
            ),
        },
        "command" => match asset.conflict.as_str() {
            "none" => format!(
                "New command '{}' — copies to {}.",
                asset.name, asset.target_path
            ),
            "skip-existing" => format!(
                "Command '{}' already exists with identical content — nothing to do.",
                asset.name
            ),
            _ => format!(
                "Command '{}' exists and differs — your conflict choice decides overwrite vs \
                 rename to '{}-imported'.",
                asset.name, asset.name
            ),
        },
        "memory" => {
            if asset.conflict == "skip-existing" {
                "An identical project-memory entry already exists — nothing to do.".to_string()
            } else {
                format!(
                    "Adds one project-memory entry ({} chars) for project '{}' — editable in the \
                     Memory page; Shannon keeps reading the original file natively.",
                    source_chars(asset),
                    roots.project.display()
                )
            }
        }
        "settings-rules" => {
            let profile_name = format!("{}-imported", source.as_str());
            match asset.conflict.as_str() {
                "skip-existing" => format!(
                    "Profile '{profile_name}' already exists with identical rules — nothing to do."
                ),
                "overwrite" => format!(
                    "Profile '{profile_name}' exists and differs — your conflict choice decides \
                     overwrite vs rename."
                ),
                _ => format!(
                    "Creates permission profile '{profile_name}' from the source allow rules \
                     (stored verbatim; inactive until you enable it in Settings → Permissions)."
                ),
            }
        }
        other => format!("Unsupported asset kind {other:?}."),
    }
}

fn summarize_source_mcp(asset: &MigrationAsset) -> String {
    let path = PathBuf::from(&asset.source_path);
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(spec) = value.get("mcpServers").and_then(|s| s.get(&asset.name)) {
                let command = spec.get("command").and_then(|c| c.as_str()).unwrap_or("");
                let args: Vec<String> = spec
                    .get("args")
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                return format!("{command} {}", args.join(" "));
            }
        }
    }
    "(source unreadable)".to_string()
}

fn source_chars(asset: &MigrationAsset) -> usize {
    std::fs::read_to_string(PathBuf::from(&asset.source_path))
        .map(|c| c.chars().count())
        .unwrap_or(0)
}

// ─── Apply ──────────────────────────────────────────────────────────────────

fn apply_core(
    source: MigrationSource,
    items: &[MigrationItemInput],
    roots: &Roots,
) -> Result<MigrationApplyReport, String> {
    let scan = scan_core(source, roots);
    let mut report = MigrationApplyReport {
        imported: 0,
        skipped: 0,
        failed: Vec::new(),
    };
    for requested in items {
        if requested.action != "import" && requested.action != "skip" {
            report.failed.push(MigrationApplyFailure {
                id: requested.id.clone(),
                error: format!(
                    "invalid action {:?} — expected \"import\" or \"skip\"",
                    requested.action
                ),
            });
            continue;
        }
        if requested.action == "skip" {
            report.skipped += 1;
            continue;
        }
        let conflict_choice = requested.conflict.as_deref().unwrap_or("rename");
        if !matches!(conflict_choice, "overwrite" | "rename" | "skip") {
            report.failed.push(MigrationApplyFailure {
                id: requested.id.clone(),
                error: format!(
                    "invalid conflict {conflict_choice:?} — expected \"overwrite\", \"rename\" or \"skip\""
                ),
            });
            continue;
        }
        let Some(asset) = scan.items.iter().find(|a| a.id == requested.id) else {
            report.failed.push(MigrationApplyFailure {
                id: requested.id.clone(),
                error: "unknown item id — re-run the scan".to_string(),
            });
            continue;
        };
        let outcome = import_asset(asset, source, conflict_choice, roots);
        match outcome {
            Ok(true) => report.imported += 1,
            Ok(false) => report.skipped += 1,
            Err(e) => report.failed.push(MigrationApplyFailure {
                id: requested.id.clone(),
                error: e,
            }),
        }
    }
    Ok(report)
}

/// Import one asset. `Ok(true)` = imported, `Ok(false)` = skipped (conflict
/// handling), `Err` = this item failed (never aborts the batch).
fn import_asset(
    asset: &MigrationAsset,
    source: MigrationSource,
    conflict_choice: &str,
    roots: &Roots,
) -> Result<bool, String> {
    match asset.kind.as_str() {
        "mcp" => import_mcp(asset, conflict_choice, roots),
        "skill" => import_skill(asset, conflict_choice, roots),
        "command" => import_command(asset, conflict_choice, roots),
        "memory" => import_memory(asset, source, roots),
        "settings-rules" => import_settings_rules(asset, source, conflict_choice, roots),
        other => Err(format!("unsupported asset kind {other:?}")),
    }
}

fn import_mcp(
    asset: &MigrationAsset,
    conflict_choice: &str,
    roots: &Roots,
) -> Result<bool, String> {
    let spec = read_source_mcp_spec(asset)?;
    let mut store = load_mcp_store(roots);
    let existing = store.iter_mut().find(|s| s.name == asset.name);
    match existing {
        Some(cur) => {
            if cur.command == spec.command && cur.args == spec.args && cur.env == spec.env {
                return Ok(false); // identical — idempotent no-op
            }
            match conflict_choice {
                "overwrite" => {
                    *cur = spec;
                }
                "rename" => {
                    let renamed = format!("{}-imported", asset.name);
                    if store.iter().any(|s| s.name == renamed) {
                        return Ok(false); // renamed slot already populated — skip
                    }
                    store.push(McpServerConfig {
                        name: renamed,
                        ..spec
                    });
                }
                _ => return Ok(false), // "skip"
            }
        }
        None => store.push(spec),
    }
    save_mcp_store(roots, &store)
}

fn import_skill(
    asset: &MigrationAsset,
    conflict_choice: &str,
    roots: &Roots,
) -> Result<bool, String> {
    let src = PathBuf::from(&asset.source_path);
    let skills_dir = shannon_skills_dir(roots);
    let mut target_name = asset.name.clone();
    if let Some(existing_md) = target_variant(&skills_dir.join(&target_name).join("SKILL.md")) {
        let incoming = std::fs::read_to_string(src.join("SKILL.md"))
            .map_err(|e| format!("read source SKILL.md: {e}"))?;
        if existing_md == incoming {
            return Ok(false); // identical — idempotent no-op
        }
        match conflict_choice {
            "overwrite" => {}
            "rename" => {
                target_name = format!("{}-imported", asset.name);
                if let Some(renamed_md) =
                    target_variant(&skills_dir.join(&target_name).join("SKILL.md"))
                {
                    if renamed_md == incoming {
                        return Ok(false); // already imported under the renamed slot
                    }
                    return Err(format!(
                        "renamed target '{target_name}' also exists with different content"
                    ));
                }
            }
            _ => return Ok(false), // "skip"
        }
    }
    let target = skills_dir.join(&target_name);
    copy_dir_recursive(&src, &target)?;
    Ok(true)
}

fn import_command(
    asset: &MigrationAsset,
    conflict_choice: &str,
    roots: &Roots,
) -> Result<bool, String> {
    let src = PathBuf::from(&asset.source_path);
    let incoming = std::fs::read_to_string(&src).map_err(|e| format!("read source: {e}"))?;
    let commands_dir = shannon_commands_dir(roots);
    let rel = format!("{}.md", asset.name);
    let mut target = commands_dir.join(&rel);
    if let Some(existing) = target_variant(&target) {
        if existing == incoming {
            return Ok(false); // identical — idempotent no-op
        }
        match conflict_choice {
            "overwrite" => {}
            "rename" => {
                let renamed = format!("{}-imported.md", asset.name);
                target = commands_dir.join(&renamed);
                if target_variant(&target).is_some() {
                    return Ok(false); // renamed slot already populated — skip
                }
            }
            _ => return Ok(false), // "skip"
        }
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(&target, incoming).map_err(|e| format!("write {}: {e}", target.display()))?;
    Ok(true)
}

/// CLAUDE.md / AGENTS.md → one `MemoryStore` entry (category `context`,
/// project = the current working directory — the same project key the engine
/// uses for memory injection). `add_or_update` dedups near-identical content
/// so re-imports never duplicate.
fn import_memory(
    asset: &MigrationAsset,
    source: MigrationSource,
    roots: &Roots,
) -> Result<bool, String> {
    let content = std::fs::read_to_string(PathBuf::from(&asset.source_path))
        .map_err(|e| format!("read source: {e}"))?;
    let project = roots.project.display().to_string();
    if memory_entry_exists(&content, &project, roots) {
        return Ok(false); // identical entry already present — idempotent no-op
    }
    let mut store = MemoryStore::new(shannon_memories_dir(roots));
    store.load().map_err(|e| e.to_string())?;
    let mut entry = MemoryEntry::with_confidence(
        &project,
        MemoryCategory::Context,
        &content,
        1.0,
        vec![
            "imported".to_string(),
            source.as_str().to_string(),
            asset.name.clone(),
        ],
    )
    .map_err(|e| e.to_string())?;
    entry.confidence = 1.0;
    // P2-4 provenance: imported entries are tagged "import" (no session id).
    entry.source_kind = Some(MemoryEntry::SOURCE_IMPORT.to_string());
    // `add_or_update` merges into a near-duplicate when one exists (>0.8
    // similar, same project + category), so re-imports never duplicate rows;
    // exact duplicates were already short-circuited above.
    store.add_or_update(entry).map_err(|e| e.to_string())?;
    Ok(true)
}

fn import_settings_rules(
    asset: &MigrationAsset,
    source: MigrationSource,
    conflict_choice: &str,
    roots: &Roots,
) -> Result<bool, String> {
    let rules = {
        let mut result = MigrationScanResult::empty(source);
        read_settings_allow_rules(Path::new(&asset.source_path), &mut result)
    };
    if rules.is_empty() {
        return Err("source allow rules vanished — re-run the scan".to_string());
    }
    let profile_name = format!("{}-imported", source.as_str());
    let rendered = render_profile_toml(&profile_name, &rules);
    let dir = shannon_profiles_dir(roots);
    let mut file_name = format!("{profile_name}.toml");
    if let Ok(existing) = std::fs::read_to_string(dir.join(&file_name)) {
        if existing == rendered {
            return Ok(false); // identical — idempotent no-op
        }
        match conflict_choice {
            "overwrite" => {}
            "rename" => {
                file_name = format!("{profile_name}-imported.toml");
                if std::fs::read_to_string(dir.join(&file_name)).is_ok() {
                    return Ok(false); // renamed slot already populated — skip
                }
            }
            _ => return Ok(false), // "skip"
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let target = dir.join(&file_name);
    std::fs::write(&target, rendered).map_err(|e| format!("write {}: {e}", target.display()))?;
    Ok(true)
}

// ─── Shared helpers ─────────────────────────────────────────────────────────

fn asset_id(source: &str, kind: &str, name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    format!("{source}:{kind}:{slug}")
}

/// `none` | `skip-existing` | `overwrite` by comparing source and target
/// file contents. Missing target ⇒ `none`; identical ⇒ `skip-existing`.
fn content_conflict(src: &Path, target: &Path) -> &'static str {
    match target_variant(target) {
        None => "none",
        Some(existing) => match std::fs::read_to_string(src) {
            Ok(incoming) if incoming == existing => "skip-existing",
            Ok(_) => "overwrite",
            Err(_) => "overwrite",
        },
    }
}

/// Read a target file if it exists (missing file → `None`, other read errors
/// are surfaced as a differing target so the conflict flow handles them).
fn target_variant(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some(String::new()), // unreadable target — treat as differing
    }
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // symlink_metadata never follows links, so a link contributes only
            // its own size and a link to a directory is not traversed.
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => stack.push(path),
                Ok(meta) => total += meta.len(),
                Err(_) => {}
            }
        }
    }
    total
}

/// Reject symlinks anywhere inside `dir` (including `dir` itself), **before
/// anything is written**: a source-side link must never make us read or copy
/// content outside the whitelisted source roots. The affected import fails
/// with a clear error; the user removes the link and re-imports.
fn reject_symlinks(dir: &Path) -> Result<(), String> {
    let meta =
        std::fs::symlink_metadata(dir).map_err(|e| format!("stat {}: {e}", dir.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "refusing to follow symlink {} — remove it from the source and re-import",
            dir.display()
        ));
    }
    if !meta.is_dir() {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let entry_meta = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("stat {}: {e}", path.display()))?;
        if entry_meta.file_type().is_symlink() {
            return Err(format!(
                "refusing to follow symlink {} — remove it from the source and re-import",
                path.display()
            ));
        }
        if entry_meta.is_dir() {
            reject_symlinks(&path)?;
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, target: &Path) -> Result<(), String> {
    // Pre-flight: bail out before any write so a symlink deeper in the tree
    // can never leave a partially-copied target behind.
    reject_symlinks(src)?;
    std::fs::create_dir_all(target).map_err(|e| format!("create {}: {e}", target.display()))?;
    let entries = std::fs::read_dir(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let from = entry.path();
        let to = target.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("copy {} → {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Exact-content existence check in the target project's memory entries.
fn memory_entry_exists(content: &str, project: &str, roots: &Roots) -> bool {
    let mut store = MemoryStore::new(shannon_memories_dir(roots));
    if store.load().is_err() {
        return false;
    }
    store
        .project_memories(project)
        .iter()
        .any(|e| e.content == content)
}

/// Load Shannon's MCP store JSON (same shape as `crate::config::load_mcp_servers`
/// but path-injectable so tests never touch the real `~`).
fn load_mcp_store(roots: &Roots) -> Vec<McpServerConfig> {
    let path = mcp_store_path(roots);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_mcp_store(roots: &Roots, servers: &[McpServerConfig]) -> Result<bool, String> {
    let path = mcp_store_path(roots);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(servers).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
    crate::file_permissions::restrict_to_owner(&path);
    Ok(true)
}

/// Extract the importable `McpServerConfig` for one asset from its source file.
fn read_source_mcp_spec(asset: &MigrationAsset) -> Result<McpServerConfig, String> {
    let text = std::fs::read_to_string(PathBuf::from(&asset.source_path))
        .map_err(|e| format!("read source: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid JSON: {e}"))?;
    let spec = value
        .get("mcpServers")
        .and_then(|s| s.get(&asset.name))
        .ok_or_else(|| {
            format!(
                "server {:?} vanished from source — re-run the scan",
                asset.name
            )
        })?;
    let command = spec
        .get("command")
        .and_then(|c| c.as_str())
        .filter(|c| !c.is_empty())
        .ok_or_else(|| format!("server {:?} has no command transport", asset.name))?
        .to_string();
    let args: Vec<String> = spec
        .get("args")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let mut env = std::collections::HashMap::new();
    if let Some(map) = spec.get("env").and_then(|e| e.as_object()) {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                env.insert(k.clone(), s.to_string());
            }
        }
    }
    Ok(McpServerConfig {
        name: asset.name.clone(),
        command,
        args,
        env,
        enabled: true,
    })
}

/// Render a custom permission profile TOML (same format
/// `shannon_engine::custom_profiles::CustomProfileRegistry::parse_file` reads;
/// mirrors `automation_commands::render_profile_toml`).
fn render_profile_toml(name: &str, auto_approve: &[String]) -> String {
    fn basic_string(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }
    fn string_array(items: &[String]) -> String {
        let inner: Vec<String> = items.iter().map(|i| basic_string(i)).collect();
        format!("[{}]", inner.join(", "))
    }
    format!(
        "# Custom permission profile — imported by the Shannon migration wizard.\n\n\
         name = {name}\n\
         description = \"Imported allow rules (unverified — review before activating).\"\n\
         auto_approve = {rules}\n\
         confirm = []\n\
         deny = []\n",
        name = basic_string(name),
        rules = string_array(auto_approve),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Test fixtures ──────────────────────────────────────────────────────

    fn temp_roots(_tag: &str) -> (tempfile::TempDir, Roots) {
        let dir = tempfile::tempdir().expect("tempdir");
        let roots = Roots {
            home: dir.path().join("home"),
            project: dir.path().join("proj"),
        };
        std::fs::create_dir_all(&roots.home).expect("home dir");
        std::fs::create_dir_all(&roots.project).expect("project dir");
        (dir, roots)
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, contents).expect("write fixture");
    }

    const SKILL_A: &str = "---\nname: commit\ndescription: Make a commit\n---\n\nCommit well.\n";
    const SKILL_B: &str = "---\nname: review\ndescription: Review code\n---\n\nReview hard.\n";
    const PROJECT_MD: &str = "# Project instructions\n\nAlways answer concisely.\n";

    /// A rich Claude Code tree: settings (rules + mcp), global mcp, project
    /// .mcp.json, two skills, two commands (one nested), project CLAUDE.md.
    fn seed_claude_code(roots: &Roots) {
        let src_home = roots.source_home(MigrationSource::ClaudeCode);
        write(
            &src_home.join("settings.json"),
            r#"{
                "permissions": {"allow": ["Bash(npm run lint)", "Read(~/.zshrc)"]},
                "mcpServers": {
                    "from-settings": {"command": "npx", "args": ["-y", "settings-mcp"]}
                }
            }"#,
        );
        write(
            &roots.home.join(".claude.json"),
            r#"{"mcpServers": {"github": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"]}}}"#,
        );
        write(
            &roots.project.join(".mcp.json"),
            r#"{"mcpServers": {"fs": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]}}}"#,
        );
        write(&src_home.join("skills/commit/SKILL.md"), SKILL_A);
        write(&src_home.join("skills/review/SKILL.md"), SKILL_B);
        write(
            &src_home.join("commands/deploy.md"),
            "Deploy the service.\n",
        );
        write(
            &src_home.join("commands/blog/draft.md"),
            "Draft a blog post.\n",
        );
        write(&roots.project.join("CLAUDE.md"), PROJECT_MD);
    }

    fn ids(result: &MigrationScanResult) -> Vec<String> {
        result.items.iter().map(|a| a.id.clone()).collect()
    }

    fn find<'a>(result: &'a MigrationScanResult, id: &str) -> &'a MigrationAsset {
        result
            .items
            .iter()
            .find(|a| a.id == id)
            .unwrap_or_else(|| panic!("asset {id} not in scan"))
    }

    // ─── Source whitelist ───────────────────────────────────────────────────

    #[test]
    fn parse_source_rejects_arbitrary_values() {
        // The source argument is a whitelist lookup — arbitrary strings
        // (including anything path-like) must be rejected outright.
        assert_eq!(
            MigrationSource::parse("claude-code").unwrap(),
            MigrationSource::ClaudeCode
        );
        assert_eq!(
            MigrationSource::parse("zcode").unwrap(),
            MigrationSource::ZCode
        );
        for evil in [
            "../../etc",
            "/home/user/.claude",
            "claude",
            "",
            "CLAUDE-CODE",
        ] {
            assert!(
                MigrationSource::parse(evil).is_err(),
                "{evil:?} must be rejected"
            );
        }
    }

    #[test]
    fn scan_never_reads_outside_the_whitelisted_roots() {
        let (_dir, roots) = temp_roots("whitelist");
        seed_claude_code(&roots);
        // Canary files the scanner has no business touching.
        write(&roots.home.join("secret.txt"), "canary");
        write(&roots.project.join("notes.md"), "canary");
        write(&roots.project.join("AGENTS.md"), "wrong source");

        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        assert!(!result.items.is_empty());
        let src_home = roots.source_home(MigrationSource::ClaudeCode);
        let global_claude_json = roots.home.join(".claude.json");
        for asset in &result.items {
            let p = Path::new(&asset.source_path);
            let whitelisted = p.starts_with(&src_home)
                || p.starts_with(&roots.project)
                || p.starts_with(&global_claude_json);
            assert!(
                whitelisted,
                "source path {} outside whitelist",
                asset.source_path
            );
        }
        // The canaries were not picked up as assets.
        assert!(!ids(&result).iter().any(|id| id.contains("secret")));
        assert!(
            !result
                .items
                .iter()
                .any(|a| a.kind == "memory" && a.name == "AGENTS.md")
        );
        assert!(std::fs::read_to_string(roots.home.join("secret.txt")).unwrap() == "canary");
    }

    #[test]
    fn symlinked_skill_directory_escaping_home_is_filtered() {
        let (_dir, roots) = temp_roots("symlink");
        let outside = tempfile::tempdir().expect("outside tempdir");
        write(&outside.path().join("SKILL.md"), "---\nname: evil\n---\n");
        let src_home = roots.source_home(MigrationSource::ClaudeCode);
        std::fs::create_dir_all(src_home.join("skills")).expect("skills dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), src_home.join("skills/evil")).expect("symlink");
        // The linked-in content lives outside the whitelisted roots — it must
        // not surface as an importable asset.
        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        assert!(
            !result.items.iter().any(|a| a.kind == "skill"),
            "symlink escape must be filtered: {:?}",
            result.items
        );
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.error.contains("whitelisted"))
        );
    }

    #[test]
    fn symlink_into_home_but_outside_source_root_is_rejected() {
        // The whitelist must be the *exact* source roots (~/.claude, project),
        // not "anywhere under $HOME": a skills-dir link pointing at another
        // home location is still an escape.
        let (_dir, roots) = temp_roots("symlink-home");
        let stash = roots.home.join("private-notes");
        write(
            &stash.join("SKILL.md"),
            "---\nname: home-evil\n---\nsecret body\n",
        );
        let src_home = roots.source_home(MigrationSource::ClaudeCode);
        std::fs::create_dir_all(src_home.join("skills")).expect("skills dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&stash, src_home.join("skills/home-evil")).expect("symlink");

        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        assert!(
            !result.items.iter().any(|a| a.kind == "skill"),
            "home-internal symlink escape must be filtered: {:?}",
            result.items
        );
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.error.contains("whitelisted")),
            "{:?}",
            result.errors
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_symlink_inside_skill_is_not_followed_and_import_fails() {
        // A file-level link inside an otherwise legit skill must not be
        // followed at apply time: the item fails, nothing is written to the
        // target (pre-flight rejection → no partial copy), and the linked-to
        // content never lands in ~/.shannon.
        let (_dir, roots) = temp_roots("symlink-file");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let secret = outside.path().join("secret.txt");
        write(&secret, "top secret\n");
        seed_claude_code(&roots);
        let skill_dir = roots
            .source_home(MigrationSource::ClaudeCode)
            .join("skills/commit");
        std::os::unix::fs::symlink(&secret, skill_dir.join("notes.md")).expect("symlink");

        // Scan still offers the skill (the top-level dir is real and inside
        // the whitelist); apply must fail that one item.
        let scan = scan_core(MigrationSource::ClaudeCode, &roots);
        assert!(
            scan.items
                .iter()
                .any(|a| a.id == "claude-code:skill:commit")
        );

        let items = vec![MigrationItemInput {
            id: "claude-code:skill:commit".into(),
            action: "import".into(),
            conflict: None,
        }];
        let report = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply");
        assert_eq!(report.imported, 0, "{report:?}");
        assert_eq!(report.failed.len(), 1);
        assert!(report.failed[0].id == "claude-code:skill:commit");
        assert!(
            report.failed[0].error.contains("symlink"),
            "error must name the symlink: {}",
            report.failed[0].error
        );
        // Pre-flight rejection: no partial target directory at all.
        assert!(
            !shannon_skills_dir(&roots).join("commit").exists(),
            "no partial copy may be left behind"
        );
    }

    #[cfg(unix)]
    #[test]
    fn memory_asset_symlink_escape_is_rejected_before_read() {
        // A project-memory file that is itself a symlink pointing outside the
        // whitelisted roots: whitelist runs BEFORE the content read, so the
        // file is rejected (and never read), not imported.
        let (_dir, roots) = temp_roots("symlink-memory");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let secret = outside.path().join("CLAUDE.md");
        write(&secret, "stolen instructions\n");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&secret, roots.project.join("CLAUDE.md")).expect("symlink");

        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        assert!(
            !result.items.iter().any(|a| a.kind == "memory"),
            "symlinked memory file must be filtered: {:?}",
            result.items
        );
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.error.contains("whitelisted")),
            "{:?}",
            result.errors
        );
    }

    // ─── Scan: claude-code ─────────────────────────────────────────────────

    #[test]
    fn scan_claude_code_full_tree_finds_all_five_kinds() {
        let (_dir, roots) = temp_roots("cc-full");
        seed_claude_code(&roots);

        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        assert_eq!(result.source, "claude-code");
        let found = ids(&result);
        for expected in [
            "claude-code:settings-rules:settings-json",
            "claude-code:mcp:from-settings",
            "claude-code:mcp:github",
            "claude-code:mcp:fs",
            "claude-code:skill:commit",
            "claude-code:skill:review",
            "claude-code:command:deploy",
            "claude-code:command:blog-draft",
            "claude-code:memory:project-memory",
        ] {
            assert!(
                found.contains(&expected.to_string()),
                "missing {expected} in {found:?}"
            );
        }
        // All fresh targets → no conflicts.
        assert!(result.items.iter().all(|a| a.conflict == "none"));
        assert!(result.not_found.is_empty(), "{:?}", result.not_found);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // Shape spot-checks on one asset.
        let skill = find(&result, "claude-code:skill:commit");
        assert_eq!(skill.kind, "skill");
        assert_eq!(skill.name, "commit");
        assert!(skill.source_path.ends_with("skills/commit"));
        assert!(skill.target_path.ends_with(".shannon/skills/commit"));
        assert!(skill.size_hint > 0);
        let rules = find(&result, "claude-code:settings-rules:settings-json");
        assert!(rules.source_path.ends_with(".claude/settings.json"));
        assert!(
            rules
                .target_path
                .ends_with(".shannon/profiles/claude-code-imported.toml")
        );
    }

    #[test]
    fn scan_missing_source_is_empty_result_not_error() {
        let (_dir, roots) = temp_roots("cc-empty");
        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        assert!(result.items.is_empty());
        assert!(result.errors.is_empty());
        // Every probed slot is reported as 未发现 / not found.
        assert!(
            result
                .not_found
                .iter()
                .any(|s| s.starts_with("skills — ") && s.contains(".claude/skills"))
        );
        assert!(
            result
                .not_found
                .iter()
                .any(|s| s.starts_with("commands — "))
        );
        assert!(
            result
                .not_found
                .iter()
                .any(|s| s.starts_with("settings — "))
        );
        assert!(
            result
                .not_found
                .iter()
                .any(|s| s.starts_with("mcp (project) — "))
        );
        assert!(
            result
                .not_found
                .iter()
                .any(|s| s.starts_with("memory (project) — "))
        );
    }

    #[test]
    fn scan_corrupted_single_file_fails_only_that_slot() {
        let (_dir, roots) = temp_roots("cc-corrupt");
        seed_claude_code(&roots);
        // Corrupt two source files; everything else must still scan.
        write(
            &roots
                .source_home(MigrationSource::ClaudeCode)
                .join("settings.json"),
            "{ not valid json !!!",
        );
        write(&roots.project.join(".mcp.json"), "]]]");

        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.path.ends_with("settings.json") && e.error.contains("invalid JSON"))
        );
        assert!(result.errors.iter().any(|e| e.path.ends_with(".mcp.json")));
        // The rest of the tree is intact.
        let found = ids(&result);
        assert!(found.contains(&"claude-code:mcp:github".to_string()));
        assert!(found.contains(&"claude-code:skill:commit".to_string()));
        assert!(found.contains(&"claude-code:memory:project-memory".to_string()));
        // The corrupted slots produced no assets.
        assert!(!found.iter().any(|id| id.contains("from-settings")));
        assert!(!found.iter().any(|id| id.contains("settings-rules")));
        assert!(found.iter().filter(|id| id.contains(":mcp:")).count() == 1);
    }

    #[test]
    fn scan_dedupes_same_name_mcp_server_by_priority() {
        let (_dir, roots) = temp_roots("cc-dup-mcp");
        let src_home = roots.source_home(MigrationSource::ClaudeCode);
        // `github` defined in all three files — project .mcp.json wins.
        write(
            &roots.project.join(".mcp.json"),
            r#"{"mcpServers": {"github": {"command": "project-cmd"}}}"#,
        );
        write(
            &roots.home.join(".claude.json"),
            r#"{"mcpServers": {"github": {"command": "global-cmd"}}}"#,
        );
        write(
            &src_home.join("settings.json"),
            r#"{"mcpServers": {"github": {"command": "settings-cmd"}}}"#,
        );
        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        let mcps: Vec<_> = result.items.iter().filter(|a| a.kind == "mcp").collect();
        assert_eq!(mcps.len(), 1, "one merged asset, got {mcps:?}");
        assert_eq!(
            mcps[0].source_path,
            roots.project.join(".mcp.json").display().to_string()
        );
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.error.contains("duplicate MCP server") && e.error.contains("github"))
        );
    }

    #[test]
    fn scan_reports_url_transport_mcp_as_error_not_asset() {
        let (_dir, roots) = temp_roots("cc-url-mcp");
        write(
            &roots.project.join(".mcp.json"),
            r#"{"mcpServers": {"remote": {"type": "sse", "url": "https://example.com/sse"}}}"#,
        );
        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        assert!(result.items.is_empty());
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.error.contains("no command transport") && e.error.contains("remote"))
        );
    }

    // ─── Scan: zcode ───────────────────────────────────────────────────────

    #[test]
    fn scan_zcode_two_level_agents_md_and_probes() {
        let (_dir, roots) = temp_roots("zcode");
        let src_home = roots.source_home(MigrationSource::ZCode);
        write(
            &src_home.join("settings.json"),
            r#"{"permissions": {"allow": ["Read"]}}"#,
        );
        write(&src_home.join("skills/commit/SKILL.md"), SKILL_A);
        write(&src_home.join("commands/deploy.md"), "Deploy.\n");
        write(&src_home.join("AGENTS.md"), "# Global agents notes\n");
        write(&roots.project.join("AGENTS.md"), PROJECT_MD);

        let result = scan_core(MigrationSource::ZCode, &roots);
        let found = ids(&result);
        // Two-level AGENTS.md (project + global) both surface as memory.
        assert!(found.contains(&"zcode:memory:project-memory".to_string()));
        assert!(found.contains(&"zcode:memory:agents-md-global".to_string()));
        let global = find(&result, "zcode:memory:agents-md-global");
        assert_eq!(global.name, "AGENTS.md (global)");
        assert!(global.source_path.contains(".zcode/AGENTS.md"));
        assert!(found.contains(&"zcode:settings-rules:settings-json".to_string()));
        assert!(found.contains(&"zcode:skill:commit".to_string()));
        assert!(found.contains(&"zcode:command:deploy".to_string()));
        // Claude-Code-style global mcp files are not probed for zcode.
        assert!(result.not_found.iter().all(|s| !s.contains(".zcode.json")));
    }

    #[test]
    fn scan_zcode_marks_absent_slots_not_found() {
        let (_dir, roots) = temp_roots("zcode-empty");
        let result = scan_core(MigrationSource::ZCode, &roots);
        assert!(result.items.is_empty());
        assert!(result.errors.is_empty());
        for slot in [
            "settings — ",
            "skills — ",
            "commands — ",
            "memory (project) — ",
            "memory (global) — ",
            "mcp (settings) — ",
        ] {
            assert!(
                result.not_found.iter().any(|s| s.starts_with(slot)),
                "slot {slot:?} not reported not-found: {:?}",
                result.not_found
            );
        }
    }

    // ─── Conflict detection ────────────────────────────────────────────────

    #[test]
    fn conflict_flags_identical_and_differing_targets() {
        let (_dir, roots) = temp_roots("conflict");
        seed_claude_code(&roots);
        // Pre-populate Shannon side: `commit` identical, `review` differs,
        // an MCP server named `fs` with a different command.
        let skills = shannon_skills_dir(&roots);
        write(&skills.join("commit/SKILL.md"), SKILL_A);
        write(
            &skills.join("review/SKILL.md"),
            "---\nname: review\n---\n\nOld content.\n",
        );
        write(
            &mcp_store_path(&roots),
            r#"[{"name":"fs","command":"old-cmd","args":[],"env":{},"enabled":true}]"#,
        );

        let result = scan_core(MigrationSource::ClaudeCode, &roots);
        assert_eq!(
            find(&result, "claude-code:skill:commit").conflict,
            "skip-existing"
        );
        assert_eq!(
            find(&result, "claude-code:skill:review").conflict,
            "overwrite"
        );
        assert_eq!(find(&result, "claude-code:mcp:fs").conflict, "overwrite");
        assert_eq!(
            find(&result, "claude-code:skill:commit").conflict,
            "skip-existing"
        );
    }

    // ─── Preview ───────────────────────────────────────────────────────────

    #[test]
    fn preview_reports_diff_summaries_per_item() {
        let (_dir, roots) = temp_roots("preview");
        seed_claude_code(&roots);
        let scan = scan_core(MigrationSource::ClaudeCode, &roots);
        let items: Vec<MigrationItemInput> = scan
            .items
            .iter()
            .map(|a| MigrationItemInput {
                id: a.id.clone(),
                action: "import".into(),
                conflict: None,
            })
            .collect();
        let preview = preview_core(MigrationSource::ClaudeCode, &items, &roots);
        assert_eq!(preview.per_item.len(), scan.items.len());
        let summary = |id: &str| {
            preview
                .per_item
                .iter()
                .find(|p| p.id == id)
                .expect("preview row")
                .diff_summary
                .clone()
        };
        assert!(summary("claude-code:skill:commit").contains("New skill 'commit'"));
        assert!(summary("claude-code:mcp:fs").contains("unverified"));
        assert!(summary("claude-code:memory:project-memory").contains("project-memory entry"));
        assert!(
            summary("claude-code:settings-rules:settings-json").contains("claude-code-imported")
        );

        // Skip actions and unknown ids get explicit wording.
        let mixed = vec![
            MigrationItemInput {
                id: "claude-code:skill:commit".into(),
                action: "skip".into(),
                conflict: None,
            },
            MigrationItemInput {
                id: "claude-code:ghost".into(),
                action: "import".into(),
                conflict: None,
            },
        ];
        let p2 = preview_core(MigrationSource::ClaudeCode, &mixed, &roots);
        assert!(p2.per_item[0].diff_summary.contains("Not selected"));
        assert!(p2.per_item[1].diff_summary.contains("Unknown item"));
    }

    #[test]
    fn preview_conflicting_items_name_the_choice() {
        let (_dir, roots) = temp_roots("preview-conflict");
        seed_claude_code(&roots);
        write(
            &shannon_skills_dir(&roots).join("review/SKILL.md"),
            "old and different\n",
        );
        let scan = scan_core(MigrationSource::ClaudeCode, &roots);
        let items: Vec<MigrationItemInput> = scan
            .items
            .iter()
            .map(|a| MigrationItemInput {
                id: a.id.clone(),
                action: "import".into(),
                conflict: None,
            })
            .collect();
        let preview = preview_core(MigrationSource::ClaudeCode, &items, &roots);
        let review = preview
            .per_item
            .iter()
            .find(|p| p.id == "claude-code:skill:review")
            .expect("review row");
        assert!(review.diff_summary.contains("exists and differs"));
        assert!(review.diff_summary.contains("review-imported"));
    }

    // ─── Apply ─────────────────────────────────────────────────────────────

    #[test]
    fn apply_claude_code_imports_everything_then_reapply_is_idempotent() {
        let (_dir, roots) = temp_roots("apply-idempotent");
        seed_claude_code(&roots);
        let scan = scan_core(MigrationSource::ClaudeCode, &roots);
        let items: Vec<MigrationItemInput> = scan
            .items
            .iter()
            .map(|a| MigrationItemInput {
                id: a.id.clone(),
                action: "import".into(),
                conflict: None,
            })
            .collect();

        let first = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply 1");
        assert_eq!(
            first.imported,
            scan.items.len(),
            "first run imports all: {first:?}"
        );
        assert!(first.failed.is_empty(), "{:?}", first.failed);

        // Destinations really landed.
        assert!(shannon_skills_dir(&roots).join("commit/SKILL.md").is_file());
        assert!(shannon_commands_dir(&roots).join("deploy.md").is_file());
        assert!(shannon_commands_dir(&roots).join("blog/draft.md").is_file());
        assert!(mcp_store_path(&roots).is_file());
        assert!(
            shannon_profiles_dir(&roots)
                .join("claude-code-imported.toml")
                .is_file()
        );
        let store = load_mcp_store(&roots);
        assert_eq!(store.len(), 3, "three MCP servers merged: {store:?}");

        // Memory entry landed in the MemoryStore under the project key.
        let mut mem = MemoryStore::new(shannon_memories_dir(&roots));
        mem.load().expect("load memories");
        let project_key = roots.project.display().to_string();
        let entries = mem.project_memories(&project_key);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, PROJECT_MD);
        assert!(entries[0].tags.contains(&"imported".to_string()));
        assert!(entries[0].tags.contains(&"claude-code".to_string()));

        // Re-entry: every item now resolves through conflict handling as a
        // skip — no duplicates, no failures.
        let second = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply 2");
        assert_eq!(
            second.imported, 0,
            "re-import must not duplicate: {second:?}"
        );
        assert_eq!(second.skipped, scan.items.len());
        assert!(second.failed.is_empty(), "{:?}", second.failed);
        let store2 = load_mcp_store(&roots);
        assert_eq!(store2.len(), 3, "no duplicate MCP servers");
        let mut mem2 = MemoryStore::new(shannon_memories_dir(&roots));
        mem2.load().expect("reload memories");
        assert_eq!(
            mem2.project_memories(&project_key).len(),
            1,
            "no duplicate memory"
        );
        let skills2 = std::fs::read_dir(shannon_skills_dir(&roots)).expect("skills");
        assert_eq!(skills2.count(), 2, "no duplicate skill dirs");
    }

    #[test]
    fn apply_respects_skip_actions() {
        let (_dir, roots) = temp_roots("apply-skip");
        seed_claude_code(&roots);
        let scan = scan_core(MigrationSource::ClaudeCode, &roots);
        let items: Vec<MigrationItemInput> = scan
            .items
            .iter()
            .map(|a| MigrationItemInput {
                id: a.id.clone(),
                action: if a.kind == "skill" { "skip" } else { "import" }.into(),
                conflict: None,
            })
            .collect();
        let report = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply");
        assert_eq!(report.imported, scan.items.len() - 2);
        assert_eq!(report.skipped, 2);
        assert!(!shannon_skills_dir(&roots).join("commit").exists());
    }

    #[test]
    fn apply_conflict_rename_overwrite_and_skip_for_skills() {
        for (choice, expect_renamed) in [("rename", true), ("overwrite", false), ("skip", false)] {
            let (_dir, roots) = temp_roots("apply-conflict");
            seed_claude_code(&roots);
            write(
                &shannon_skills_dir(&roots).join("review/SKILL.md"),
                "original local edit\n",
            );
            let items = vec![MigrationItemInput {
                id: "claude-code:skill:review".into(),
                action: "import".into(),
                conflict: Some(choice.into()),
            }];
            let report = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply");
            let original =
                std::fs::read_to_string(shannon_skills_dir(&roots).join("review/SKILL.md"))
                    .expect("read target");
            if choice == "skip" {
                assert_eq!(report.skipped, 1, "{choice}");
                assert_eq!(original, "original local edit\n");
            } else {
                assert_eq!(report.imported, 1, "{choice}");
            }
            if expect_renamed {
                assert_eq!(original, "original local edit\n", "rename keeps original");
                assert_eq!(
                    std::fs::read_to_string(
                        shannon_skills_dir(&roots).join("review-imported/SKILL.md")
                    )
                    .expect("renamed copy"),
                    SKILL_B
                );
            } else if choice == "overwrite" {
                assert_eq!(original, SKILL_B, "overwrite replaces content");
            }
        }
    }

    #[test]
    fn apply_mcp_conflict_rename_merges_both_servers() {
        let (_dir, roots) = temp_roots("apply-mcp-conflict");
        seed_claude_code(&roots);
        write(
            &mcp_store_path(&roots),
            r#"[{"name":"fs","command":"old","args":[],"env":{},"enabled":true}]"#,
        );
        let items = vec![MigrationItemInput {
            id: "claude-code:mcp:fs".into(),
            action: "import".into(),
            conflict: Some("rename".into()),
        }];
        let report = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply");
        assert_eq!(report.imported, 1);
        let store = load_mcp_store(&roots);
        assert_eq!(store.len(), 2);
        let old = store.iter().find(|s| s.name == "fs").expect("old kept");
        assert_eq!(old.command, "old");
        let imported = store
            .iter()
            .find(|s| s.name == "fs-imported")
            .expect("renamed");
        assert_eq!(imported.command, "npx");
        assert_eq!(
            imported.args,
            vec!["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
        );
    }

    #[test]
    fn apply_settings_rules_profile_parses_with_registry() {
        let (_dir, roots) = temp_roots("apply-profile");
        seed_claude_code(&roots);
        let items = vec![MigrationItemInput {
            id: "claude-code:settings-rules:settings-json".into(),
            action: "import".into(),
            conflict: None,
        }];
        let report = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply");
        assert_eq!(report.imported, 1);

        let path = shannon_profiles_dir(&roots).join("claude-code-imported.toml");
        let text = std::fs::read_to_string(&path).expect("profile written");
        assert!(text.contains("Bash(npm run lint)"));
        // The written TOML must be loadable by the real profile registry.
        let def = shannon_engine::custom_profiles::CustomProfileRegistry::parse_file(&path)
            .expect("registry parses imported profile");
        assert_eq!(def.name, "claude-code-imported");
        assert_eq!(
            def.auto_approve,
            vec!["Bash(npm run lint)", "Read(~/.zshrc)"]
        );

        // Idempotent re-apply: identical profile → skipped.
        let second = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply 2");
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 1);
    }

    #[test]
    fn apply_memory_imports_as_context_entry_and_reimport_skips() {
        let (_dir, roots) = temp_roots("apply-memory");
        write(&roots.project.join("CLAUDE.md"), PROJECT_MD);
        let scan = scan_core(MigrationSource::ClaudeCode, &roots);
        assert_eq!(
            find(&scan, "claude-code:memory:project-memory").conflict,
            "none"
        );
        let items = vec![MigrationItemInput {
            id: "claude-code:memory:project-memory".into(),
            action: "import".into(),
            conflict: None,
        }];
        let first = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply");
        assert_eq!(first.imported, 1);
        // Scan now reports the identical entry as skip-existing.
        let rescan = scan_core(MigrationSource::ClaudeCode, &roots);
        assert_eq!(
            find(&rescan, "claude-code:memory:project-memory").conflict,
            "skip-existing"
        );
        let second = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply 2");
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 1);
    }

    #[test]
    fn apply_rejects_invalid_action_conflict_and_unknown_id() {
        let (_dir, roots) = temp_roots("apply-invalid");
        seed_claude_code(&roots);
        let items = vec![
            MigrationItemInput {
                id: "claude-code:skill:commit".into(),
                action: "delete".into(),
                conflict: None,
            },
            MigrationItemInput {
                id: "claude-code:skill:review".into(),
                action: "import".into(),
                conflict: Some("nuke".into()),
            },
            MigrationItemInput {
                id: "claude-code:ghost".into(),
                action: "import".into(),
                conflict: None,
            },
        ];
        let report = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply");
        assert_eq!(report.imported, 0);
        assert_eq!(report.failed.len(), 3);
        assert!(report.failed[0].error.contains("invalid action"));
        assert!(report.failed[1].error.contains("invalid conflict"));
        assert!(report.failed[2].error.contains("unknown item id"));
        assert!(!shannon_skills_dir(&roots).join("commit").exists());
    }

    #[test]
    fn apply_zcode_memory_two_level() {
        let (_dir, roots) = temp_roots("apply-zcode");
        let src_home = roots.source_home(MigrationSource::ZCode);
        write(&src_home.join("AGENTS.md"), "# Global rules\n");
        write(&roots.project.join("AGENTS.md"), "# Project rules\n");
        let scan = scan_core(MigrationSource::ZCode, &roots);
        let items: Vec<MigrationItemInput> = scan
            .items
            .iter()
            .map(|a| MigrationItemInput {
                id: a.id.clone(),
                action: "import".into(),
                conflict: None,
            })
            .collect();
        let report = apply_core(MigrationSource::ZCode, &items, &roots).expect("apply");
        assert_eq!(report.imported, 2);
        let mut mem = MemoryStore::new(shannon_memories_dir(&roots));
        mem.load().expect("load");
        let entries = mem.project_memories(&roots.project.display().to_string());
        assert_eq!(entries.len(), 2);
        let contents: Vec<_> = entries.iter().map(|e| e.content.as_str()).collect();
        assert!(contents.contains(&"# Global rules\n"));
        assert!(contents.contains(&"# Project rules\n"));
    }

    #[test]
    fn apply_memory_imports_are_tagged_import() {
        // P2-4 provenance: migration-imported memories carry
        // source_kind = "import" and no source session.
        let tmp = tempfile::tempdir().expect("tmp");
        let roots = Roots {
            home: tmp.path().join("home"),
            project: tmp.path().join("proj"),
        };
        std::fs::create_dir_all(&roots.project).expect("project dir");
        write(&roots.project.join("CLAUDE.md"), "# rules\n");
        let scan = scan_core(MigrationSource::ClaudeCode, &roots);
        let items: Vec<MigrationItemInput> = scan
            .items
            .iter()
            .filter(|a| a.kind == "memory")
            .map(|a| MigrationItemInput {
                id: a.id.clone(),
                action: "import".into(),
                conflict: None,
            })
            .collect();
        assert_eq!(items.len(), 1);
        let report = apply_core(MigrationSource::ClaudeCode, &items, &roots).expect("apply");
        assert_eq!(report.imported, 1);
        let mut mem = MemoryStore::new(shannon_memories_dir(&roots));
        mem.load().expect("load");
        let entries = mem.project_memories(&roots.project.display().to_string());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_kind.as_deref(), Some("import"));
        assert!(entries[0].source_session_id.is_none());
    }

    // ─── Frozen DTO wire shapes ────────────────────────────────────────────

    #[test]
    fn frozen_dto_wire_shapes_are_camel_case() {
        let asset = MigrationAsset {
            id: "claude-code:skill:commit".into(),
            kind: "skill".into(),
            name: "commit".into(),
            source_path: "/h/.claude/skills/commit".into(),
            target_path: "/h/.shannon/skills/commit".into(),
            conflict: "none".into(),
            size_hint: 12,
        };
        let scan = MigrationScanResult {
            source: "claude-code".into(),
            items: vec![asset],
            not_found: vec!["skills — /h/.claude/skills".into()],
            errors: vec![MigrationScanError {
                path: "/h/x".into(),
                error: "bad".into(),
            }],
        };
        let json = serde_json::to_value(&scan).expect("serialize");
        let obj = json.as_object().expect("object");
        // serde_json maps sort keys — compare the key *set*.
        let mut keys: Vec<_> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["errors", "items", "notFound", "source"]);
        let item = &obj["items"][0];
        let mut item_keys: Vec<_> = item
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        item_keys.sort_unstable();
        assert_eq!(
            item_keys,
            vec![
                "conflict",
                "id",
                "kind",
                "name",
                "sizeHint",
                "sourcePath",
                "targetPath"
            ]
        );

        let report = MigrationApplyReport {
            imported: 1,
            skipped: 2,
            failed: vec![MigrationApplyFailure {
                id: "x".into(),
                error: "e".into(),
            }],
        };
        let rj = serde_json::to_value(&report).expect("serialize");
        let mut report_keys: Vec<_> = rj.as_object().unwrap().keys().map(String::as_str).collect();
        report_keys.sort_unstable();
        assert_eq!(report_keys, vec!["failed", "imported", "skipped"]);
        let mut failure_keys: Vec<_> = rj["failed"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        failure_keys.sort_unstable();
        assert_eq!(failure_keys, vec!["error", "id"]);

        let preview = MigrationPreviewResult {
            per_item: vec![MigrationPreviewItem {
                id: "x".into(),
                diff_summary: "s".into(),
            }],
        };
        let pj = serde_json::to_value(&preview).expect("serialize");
        let mut preview_keys: Vec<_> = pj.as_object().unwrap().keys().map(String::as_str).collect();
        preview_keys.sort_unstable();
        assert_eq!(preview_keys, vec!["perItem"]);
        let mut row_keys: Vec<_> = pj["perItem"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        row_keys.sort_unstable();
        assert_eq!(row_keys, vec!["diffSummary", "id"]);
    }

    #[test]
    fn migration_item_input_deserializes_frozen_and_additive_fields() {
        let frozen: MigrationItemInput =
            serde_json::from_str(r#"{"id": "a", "action": "import"}"#).expect("frozen shape");
        assert_eq!(frozen.id, "a");
        assert_eq!(frozen.action, "import");
        assert_eq!(frozen.conflict, None);
        let additive: MigrationItemInput =
            serde_json::from_str(r#"{"id": "a", "action": "import", "conflict": "rename"}"#)
                .expect("additive conflict field");
        assert_eq!(additive.conflict.as_deref(), Some("rename"));
    }
}
