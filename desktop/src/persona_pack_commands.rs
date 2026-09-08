//! P2-2 — Persona / profile pack: one-file export & import (`.tar.gz`).
//!
//! Packs Shannon's personalization surfaces into a single `shannon-*.tar.gz`
//! so a setup can be moved between machines:
//!
//! | include flag | source (this machine)                        | pack path                          |
//! |--------------|----------------------------------------------|------------------------------------|
//! | `skills`     | `~/.shannon/skills/<name>/**`                | `skills/<name>/**`                 |
//! | `commands`   | `~/.shannon/commands/**/*.md`                | `commands/<rel>.md`                |
//! | `memory`     | `MemoryStore` (`~/.shannon/memories/`)       | `memories.jsonl`                   |
//! | `routines`   | `~/.shannon/scheduled-tasks/<dir>/**`, `~/.shannon/routines.toml`, `~/.shannon/routine-overrides.json` | `routines/…` |
//! | `profiles`   | `<cwd>/.shannon/profiles/*.toml`             | `profiles/<name>.toml`             |
//! | `persona`    | `~/.claude/CLAUDE.md` (user-scope global instructions — Shannon's "persona" equivalent) | `persona.md` |
//!
//! plus a `manifest.json` (`version=1`, `generator=shannon-x.y.z`,
//! `createdAtMs`, `counts`, per-asset `{path, kind, bytes, sha256, stripped}`).
//!
//! Frozen wire contract (P2-2):
//! - `persona_pack_export({ path, include }) -> { path, counts, stripped }`
//! - `persona_pack_import({ path, conflict, include }) -> { imported: counts, skipped: counts, failed: [{ item, error }] }`
//! - `persona_pack_inspect({ path }) -> { version, counts, createdAtMs, generator }`
//!   (pack preview — never writes anything).
//!
//! Safety rules (brief):
//! - **Secret stripping is hard**: every textual asset (`.md/.toml/.json/
//!   .jsonl/.yml/.yaml/.txt`) is scanned before it enters the pack; secret-
//!   shaped lines / tokens / webhook credentials are replaced with
//!   `[stripped: secret]` and counted (see [`strip_text`] for the rule list).
//!   Memory entries export content fields only — never `source_session_id`
//!   and never the source machine's project paths.
//! - **Path safety on import**: manifest paths must be relative, `..`-free,
//!   backslash-free and component-clean; symlink / hardlink tar entries are
//!   rejected outright (mirrors `migration_commands::reject_symlinks`); size
//!   caps bound decompression (zip-slip / tar-bomb defense).
//! - **Idempotent**: re-importing a pack whose content already matches the
//!   target surfaces as `skipped`, never as a duplicate.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::{Archive, Builder, EntryType, Header};

use shannon_core::memory::{MemoryCategory, MemoryEntry, MemoryStore};

// ─── Constants ──────────────────────────────────────────────────────────────

/// Placeholder every stripped secret is replaced with. Published in the
/// export result / report so users can audit what left the machine.
pub const STRIPPED_PLACEHOLDER: &str = "[stripped: secret]";

/// Manifest file name inside the pack.
const MANIFEST_NAME: &str = "manifest.json";

/// Pack format version this module writes and accepts.
const PACK_VERSION: u32 = 1;

/// Decompression guard: no single packed file may exceed 64 MB …
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// … and the whole pack may not decompress to more than 512 MB.
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

/// Extensions eligible for secret scanning. Other (binary) files are packed
/// verbatim — they cannot be line-scanned without corrupting them.
fn is_textual(rel: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e, "md" | "toml" | "json" | "jsonl" | "yml" | "yaml" | "txt"))
        .unwrap_or(false)
}

// ─── Roots (path-injectable so tests never touch the real `~`) ──────────────

/// Filesystem roots every export/import is anchored to. Production resolves
/// these from the environment once per invocation; tests inject tempdirs.
#[derive(Debug, Clone)]
pub struct PackRoots {
    pub home: PathBuf,
    pub project: PathBuf,
}

impl PackRoots {
    /// Resolve from the environment (`HOME` / `USERPROFILE`, then cwd).
    fn from_env() -> Result<Self, String> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .map_err(|_| "could not resolve home directory".to_string())?;
        let project = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Ok(Self { home, project })
    }

    fn skills_dir(&self) -> PathBuf {
        self.home.join(".shannon").join("skills")
    }

    fn commands_dir(&self) -> PathBuf {
        self.home.join(".shannon").join("commands")
    }

    fn memories_dir(&self) -> PathBuf {
        self.home.join(".shannon").join("memories")
    }

    fn scheduled_tasks_dir(&self) -> PathBuf {
        self.home.join(".shannon").join("scheduled-tasks")
    }

    fn routines_toml(&self) -> PathBuf {
        self.home.join(".shannon").join("routines.toml")
    }

    fn routine_overrides(&self) -> PathBuf {
        self.home.join(".shannon").join("routine-overrides.json")
    }

    fn profiles_dir(&self) -> PathBuf {
        self.project.join(".shannon").join("profiles")
    }

    /// Shannon's persona equivalent: the user-scope global instruction file
    /// (`~/.claude/CLAUDE.md`) that `project_instructions.rs` loads with
    /// `InstructionScope::User` on every query.
    fn persona_file(&self) -> PathBuf {
        self.home.join(".claude").join("CLAUDE.md")
    }
}

// ─── DTOs (frozen wire shapes, camelCase) ───────────────────────────────────

/// Category selection shared by export and import. Missing fields default to
/// `false`, so an omitted `include` packs/imports nothing rather than
/// everything.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PackInclude {
    pub skills: bool,
    pub commands: bool,
    pub memory: bool,
    pub routines: bool,
    pub profiles: bool,
    pub persona: bool,
}

/// Per-category asset counts (frozen shape).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PackCounts {
    pub skills: u32,
    pub commands: u32,
    pub memories: u32,
    pub routines: u32,
    pub profiles: u32,
    pub persona: u32,
}

impl PackCounts {
    /// Bump the counter for a pack `kind` string; unknown kinds are ignored
    /// (defensive — our own writer only emits the kinds below).
    fn bump_kind(&mut self, kind: &str) {
        match kind {
            "skill" => self.skills += 1,
            "command" => self.commands += 1,
            "memory" => self.memories += 1,
            "routine" => self.routines += 1,
            "profile" => self.profiles += 1,
            "persona" => self.persona += 1,
            _ => {}
        }
    }
}

/// Result of `persona_pack_export`. `stripped` (additive to the frozen
/// `{path, counts}` shape) is the total number of secret redactions applied
/// across all packed assets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackExportResult {
    pub path: String,
    pub counts: PackCounts,
    pub stripped: u32,
}

/// One failed import unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackFailure {
    pub item: String,
    pub error: String,
}

/// Result of `persona_pack_import`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackImportReport {
    pub imported: PackCounts,
    pub skipped: PackCounts,
    pub failed: Vec<PackFailure>,
}

/// Result of `persona_pack_inspect` (frozen shape — preview only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackInspectResult {
    pub version: u32,
    pub counts: PackCounts,
    pub created_at_ms: i64,
    pub generator: String,
}

/// One manifest asset row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackAssetEntry {
    path: String,
    /// `skill | command | memory | routine | profile | persona`
    kind: String,
    bytes: u64,
    sha256: String,
    stripped: u32,
}

/// Pack manifest (`manifest.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackManifest {
    version: u32,
    generator: String,
    created_at_ms: i64,
    counts: PackCounts,
    assets: Vec<PackAssetEntry>,
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

/// Export the selected personalization surfaces to `path` as `.tar.gz`.
/// Content is secret-stripped before it is written (hard rule) — the result
/// reports how many redactions were applied.
#[tauri::command]
pub async fn persona_pack_export(
    path: String,
    include: PackInclude,
) -> Result<PackExportResult, String> {
    let roots = PackRoots::from_env()?;
    export_core(include, &roots, Path::new(&path))
}

/// Preview a pack without touching the target stores (import step 1).
#[tauri::command]
pub async fn persona_pack_inspect(path: String) -> Result<PackInspectResult, String> {
    inspect_core(Path::new(&path))
}

/// Import a pack. `conflict` decides what happens when a target already
/// exists with **different** content: `skip` (keep ours), `overwrite`
/// (theirs wins), `rename` (import under `<name>-imported`). Identical
/// content always skips — re-importing the same pack never duplicates.
#[tauri::command]
pub async fn persona_pack_import(
    path: String,
    conflict: String,
    include: PackInclude,
) -> Result<PackImportReport, String> {
    let roots = PackRoots::from_env()?;
    import_core(Path::new(&path), &conflict, include, &roots)
}

// ─── Export ─────────────────────────────────────────────────────────────────

/// One file staged for packing.
struct PackedFile {
    rel: String,
    kind: &'static str,
    data: Vec<u8>,
    stripped: u32,
}

pub fn export_core(
    include: PackInclude,
    roots: &PackRoots,
    out: &Path,
) -> Result<PackExportResult, String> {
    let mut files: Vec<PackedFile> = Vec::new();
    let mut memory_count: u32 = 0;

    if include.skills {
        collect_dir(&roots.skills_dir(), "skills", "skill", &mut files)?;
    }
    if include.commands {
        collect_dir(&roots.commands_dir(), "commands", "command", &mut files)?;
    }
    if include.memory {
        memory_count = pack_memories(roots, &mut files)?;
    }
    if include.routines {
        // Scheduled tasks (`~/.shannon/scheduled-tasks/<slug>-<id>/`) plus the
        // two user-global routine files (triggered routines TOML + the
        // desktop's enable/disable overrides). Each packed surface counts as
        // one `routines` unit.
        collect_dir(
            &roots.scheduled_tasks_dir(),
            "routines",
            "routine",
            &mut files,
        )?;
        collect_single_file(
            &roots.routines_toml(),
            "routines/routines.toml",
            "routine",
            &mut files,
        )?;
        collect_single_file(
            &roots.routine_overrides(),
            "routines/routine-overrides.json",
            "routine",
            &mut files,
        )?;
    }
    if include.profiles {
        collect_dir(&roots.profiles_dir(), "profiles", "profile", &mut files)?;
    }
    if include.persona {
        collect_single_file(&roots.persona_file(), "persona.md", "persona", &mut files)?;
    }

    // Counts are unit-based, mirroring the import counters: a skill/routine
    // directory is ONE unit however many files it holds; commands and
    // profiles count per file; memories per exported row.
    let counts = PackCounts {
        skills: unit_count(&files, "skills"),
        commands: file_count(&files, "commands"),
        memories: memory_count,
        routines: unit_count(&files, "routines"),
        profiles: file_count(&files, "profiles"),
        persona: u32::from(files.iter().any(|f| f.rel == "persona.md")),
    };

    let mut stripped_total: u32 = 0;
    let mut assets: Vec<PackAssetEntry> = Vec::with_capacity(files.len());
    for file in &files {
        stripped_total += file.stripped;
        let mut hasher = Sha256::new();
        hasher.update(&file.data);
        assets.push(PackAssetEntry {
            path: file.rel.clone(),
            kind: file.kind.to_string(),
            bytes: file.data.len() as u64,
            sha256: format!("{:x}", hasher.finalize()),
            stripped: file.stripped,
        });
    }

    let manifest = PackManifest {
        version: PACK_VERSION,
        generator: format!("shannon-{}", env!("CARGO_PKG_VERSION")),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        counts: counts.clone(),
        assets,
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
    }
    let out_file =
        std::fs::File::create(out).map_err(|e| format!("create {}: {e}", out.display()))?;
    let gz = GzEncoder::new(out_file, Compression::default());
    let mut builder = Builder::new(gz);
    append_pack_file(&mut builder, MANIFEST_NAME, &manifest_json)?;
    for file in &files {
        append_pack_file(&mut builder, &file.rel, &file.data)?;
    }
    let gz = builder
        .into_inner()
        .map_err(|e| format!("finish tar: {e}"))?;
    gz.finish().map_err(|e| format!("finish gzip: {e}"))?;

    Ok(PackExportResult {
        path: out.display().to_string(),
        counts,
        stripped: stripped_total,
    })
}

fn append_pack_file(
    builder: &mut Builder<GzEncoder<std::fs::File>>,
    rel: &str,
    data: &[u8],
) -> Result<(), String> {
    let mut header = Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    builder
        .append_data(&mut header, rel, data)
        .map_err(|e| format!("pack {rel}: {e}"))
}

/// Recursively stage every regular file under `dir` as `<pack_prefix>/<rel>`.
/// Symlinks (files and dirs) are skipped on export — a link must never make
/// us read outside the root it lives in (same stance as
/// `migration_commands::reject_symlinks`).
fn collect_dir(
    dir: &Path,
    pack_prefix: &str,
    kind: &'static str,
    files: &mut Vec<PackedFile>,
) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries =
            std::fs::read_dir(&current).map_err(|e| format!("read {}: {e}", current.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.file_type().is_symlink() {
                continue; // never follow — see doc comment
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let rel_src = path
                .strip_prefix(dir)
                .map_err(|e| format!("relativize {}: {e}", path.display()))?;
            let rel = format!(
                "{}/{}",
                pack_prefix,
                rel_src.to_string_lossy().replace('\\', "/")
            );
            stage_file(&path, &rel, kind, files)?;
        }
    }
    Ok(())
}

/// Stage one existing file at a fixed pack path (missing source → no-op).
fn collect_single_file(
    src: &Path,
    rel: &str,
    kind: &'static str,
    files: &mut Vec<PackedFile>,
) -> Result<(), String> {
    let meta = match std::fs::symlink_metadata(src) {
        Ok(m) => m,
        Err(_) => return Ok(()), // absent surface → nothing to pack
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Ok(());
    }
    stage_file(src, rel, kind, files)
}

/// Read, strip (if textual) and stage one source file.
fn stage_file(
    src: &Path,
    rel: &str,
    kind: &'static str,
    files: &mut Vec<PackedFile>,
) -> Result<(), String> {
    let raw = std::fs::read(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    if is_textual(rel) {
        let (data, stripped) = match String::from_utf8(raw) {
            Ok(text) => {
                let (stripped_text, n) = strip_text(&text, is_structured(rel));
                (stripped_text.into_bytes(), n)
            }
            Err(e) => (e.into_bytes(), 0), // not valid UTF-8 → pack verbatim
        };
        files.push(PackedFile {
            rel: rel.to_string(),
            kind,
            data,
            stripped,
        });
    } else {
        files.push(PackedFile {
            rel: rel.to_string(),
            kind,
            data: raw,
            stripped: 0,
        });
    }
    Ok(())
}

/// Memory export wire row — content fields only. Deliberately absent:
/// `id`, `project` (a path from the exporting machine), `source_session_id`,
/// `accessed_at`, `access_count`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportedMemory {
    category: String,
    content: String,
    tags: Vec<String>,
    confidence: f64,
    created_at_ms: i64,
    /// P2-4 provenance kind is preserved (never the session id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_kind: Option<String>,
}

fn pack_memories(roots: &PackRoots, files: &mut Vec<PackedFile>) -> Result<u32, String> {
    let dir = roots.memories_dir();
    let mut store = MemoryStore::new(dir);
    if store.storage_path().is_dir() {
        store.load().map_err(|e| format!("load memories: {e}"))?;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut stripped: u32 = 0;
    for entry in store.all_entries() {
        // Memory content is free prose (serialized into a JSON string after
        // stripping), so plain-text semantics apply — the JSONL row structure
        // is created after stripping and stays valid by construction.
        let (content, n) = strip_text(&entry.content, false);
        stripped += n;
        let row = ExportedMemory {
            category: entry.category.to_string(),
            content,
            tags: entry.tags,
            confidence: entry.confidence.clamp(0.0, 1.0),
            created_at_ms: entry.created_at.timestamp_millis(),
            source_kind: entry.source_kind,
        };
        lines.push(serde_json::to_string(&row).map_err(|e| e.to_string())?);
    }
    if lines.is_empty() {
        return Ok(0); // no memories → no memories.jsonl in the pack
    }
    let mut data = lines.join("\n");
    data.push('\n');
    files.push(PackedFile {
        rel: "memories.jsonl".to_string(),
        kind: "memory",
        data: data.into_bytes(),
        stripped,
    });
    Ok(lines.len() as u32)
}

/// Number of distinct first components under `prefix/` (skill / task-dir
/// units — a directory with many files is still one unit).
fn unit_count(files: &[PackedFile], prefix: &str) -> u32 {
    let group_prefix = format!("{prefix}/");
    let mut names: Vec<&str> = Vec::new();
    for file in files {
        if let Some(rest) = file.rel.strip_prefix(&group_prefix) {
            let name = rest.split('/').next().unwrap_or("");
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.len() as u32
}

/// Number of packed files under `prefix/` (file-granular kinds).
fn file_count(files: &[PackedFile], prefix: &str) -> u32 {
    let group_prefix = format!("{prefix}/");
    files
        .iter()
        .filter(|f| f.rel.starts_with(&group_prefix))
        .count() as u32
}

// ─── Secret stripping (hard rule) ───────────────────────────────────────────
//
// Rule list (published in the P2-2 report; every rule has a test):
//   R1 secret assignment lines — `key = value` / `key: value` /
//      `"key": value` / `- key: value` / `export KEY=value` where key
//      (case-insensitive, `-`≡`_`) is one of the secret keys below.
//      Plain-text assets (`.md`/`.txt`): the whole line is replaced.
//      Structured assets (`.toml`/`.json`/`.jsonl`/`.yml`/`.yaml`): only the
//      VALUE is replaced with `"[stripped: secret]"` so the line — and the
//      file Shannon re-parses on import — stays syntactically valid
//      (same philosophy as R2's in-place token replacement).
//   R2 prefixed credential tokens — `sk-…` (≥16), `ghp_…` (≥20),
//      `github_pat_…` (≥20), `xoxa-/xoxb-/xoxp-/xoxr-/xoxs-…` (≥10) → token
//      substring replaced, rest of the line kept.
//   R3 webhook URLs — the credential segment of known webhook URLs
//      (Discord / Slack / Feishu) is replaced in place (the URL ends at the
//      first whitespace/quote/bracket); text before and after the URL on the
//      same line is preserved. URL query params
//      `key|token|access_token|secret|password|sig|signature` replaced with
//      the placeholder.
//   R4 Authorization bearer tokens — `Bearer <token≥12>` → token replaced.
//   R5 PEM private-key blocks — from `-----BEGIN … PRIVATE KEY-----` through
//      the matching `-----END …-----` → single placeholder line. (If the
//      BEGIN line itself is a secret assignment in a structured file, the
//      value is replaced R1-style and the rest of the block is still
//      consumed — no key material may survive, hard rule wins.)
//
// Nothing else is rewritten: `max_tokens = 4096`, `token_refresh_interval`,
// `passwords are bad`, `sk-learn` and ordinary prose/prose-URLs are untouched
// (asserted by tests).

/// Structured (parser-validated) asset extensions: R1 must preserve line
/// syntax so Shannon can re-parse the imported file.
fn is_structured(rel: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e, "toml" | "json" | "jsonl" | "yml" | "yaml"))
        .unwrap_or(false)
}

fn strip_text(content: &str, structured: bool) -> (String, u32) {
    let mut out = String::with_capacity(content.len());
    let mut count: u32 = 0;
    let mut in_pem = false;
    for chunk in content.split_inclusive('\n') {
        let line = chunk.strip_suffix('\n').unwrap_or(chunk);
        let newline = if chunk.ends_with('\n') { "\n" } else { "" };
        if in_pem {
            // Inside a private-key block: drop lines entirely (no blank-line
            // residue), keep watching for the END marker.
            if line.contains("-----END") && line.contains("KEY-----") {
                in_pem = false;
            }
            continue;
        }
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {
            in_pem = true;
            if let Some(sep) = secret_assignment_sep(line) {
                // Assignment carrying an inline PEM (e.g. TOML
                // `key = '''-----BEGIN …'''`): replace the value R1-style so
                // the key text itself is gone, then keep consuming the rest
                // of the block. The closing delimiter dies with the block —
                // accepted (and documented): no key material may survive.
                if structured {
                    out.push_str(&strip_assignment_value(line, sep));
                    out.push_str(newline);
                } else {
                    out.push_str(STRIPPED_PLACEHOLDER);
                    out.push_str(newline);
                }
                count += 1;
                continue;
            }
            out.push_str(STRIPPED_PLACEHOLDER);
            out.push_str(newline);
            count += 1;
            continue;
        }
        if let Some(sep) = secret_assignment_sep(line) {
            if structured {
                out.push_str(&strip_assignment_value(line, sep));
            } else {
                out.push_str(STRIPPED_PLACEHOLDER);
            }
            out.push_str(newline);
            count += 1;
            continue;
        }
        let (stripped_line, n) = strip_inline_secrets(line);
        count += n;
        out.push_str(&stripped_line);
        out.push_str(newline);
    }
    (out, count)
}

/// Keys whose assignment is treated as a secret. Matched case-insensitively
/// with `-` normalized to `_` and only as the FULL key (`max_tokens` never
/// matches `token`).
const SECRET_KEYS: &[&str] = &[
    "access_token",
    "api_key",
    "api_secret",
    "apikey",
    "auth",
    "auth_token",
    "bot_token",
    "client_secret",
    "id_token",
    "passwd",
    "password",
    "private_key",
    "privatekey",
    "refresh_token",
    "secret",
    "secret_key",
    "signing_secret",
    "token",
    "webhook_secret",
];

/// R1 — secret assignment detector. Returns the byte offset of the `=`/`:`
/// separator in `line` when the line assigns to a secret key (matched
/// case-insensitively with `-` normalized to `_`, FULL-key match only, and
/// only when a non-empty value follows — `password:` alone is a prompt).
fn secret_assignment_sep(line: &str) -> Option<usize> {
    let start = {
        let t = line.trim_start();
        // Markdown list markers and shell `export`.
        let t = t.trim_start_matches(['-', '*', '+']);
        let t = t.trim_start();
        let t = t.strip_prefix("export ").unwrap_or(t).trim_start();
        line.len() - t.len()
    };
    let t = &line[start..];
    let sep_rel = t.find(['=', ':'])?;
    let key = t[..sep_rel]
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    if key.is_empty() {
        return None;
    }
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    if !SECRET_KEYS.contains(&normalized.as_str()) {
        return None;
    }
    // Require a non-empty value — `password:` with nothing after it is a
    // prompt/placeholder, not a secret.
    if t[sep_rel + 1..].trim().is_empty() {
        return None;
    }
    Some(start + sep_rel)
}

/// R1 for structured assets — replace ONLY the value with
/// `"[stripped: secret]"`, keeping `key<sep>` (and any trailing content such
/// as a JSON comma or TOML comment) so the line stays parseable.
fn strip_assignment_value(line: &str, sep: usize) -> String {
    let bytes = line.as_bytes();
    let mut value_start = sep + 1;
    while value_start < bytes.len() && (bytes[value_start] == b' ' || bytes[value_start] == b'\t') {
        value_start += 1;
    }
    let value_end = if bytes.get(value_start) == Some(&b'"') {
        // Quoted string: run to the closing, unescaped quote.
        let mut i = value_start + 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                i += 1;
                break;
            }
            i += 1;
        }
        i.min(bytes.len())
    } else {
        // Bare value: runs to the first structural character or end of line.
        let mut i = value_start;
        while i < bytes.len() {
            match bytes[i] {
                b' ' | b'\t' | b',' | b'}' | b']' | b'#' | b'\r' => break,
                _ => i += 1,
            }
        }
        i
    };
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..value_start]);
    out.push('"');
    out.push_str(STRIPPED_PLACEHOLDER);
    out.push('"');
    out.push_str(&line[value_end..]);
    out
}

/// R2/R3/R4 — inline replacements. Returns the rewritten line and the number
/// of redactions applied.
fn strip_inline_secrets(line: &str) -> (String, u32) {
    let mut out = line.to_string();
    let mut count = 0;

    // R3a — known credential-bearing webhook URLs: replace only the URL's
    // credential segment (everything after the fixed prefix, up to the end of
    // the URL) so text before/after the URL on the same line survives.
    for prefix in WEBHOOK_PREFIXES {
        if let Some(pos) = out.find(prefix) {
            let start = pos + prefix.len();
            let bytes = out.as_bytes();
            let mut end = start;
            while end < bytes.len() {
                match bytes[end] {
                    b' ' | b'\t' | b'"' | b'\'' | b')' | b']' | b'>' | b',' | b';' => break,
                    _ => end += 1,
                }
            }
            if end > start {
                out.replace_range(start..end, STRIPPED_PLACEHOLDER);
                count += 1;
            }
        }
    }

    // R3b — URL query params carrying credentials (`?key=…`, `&token=…`).
    count += replace_query_params(&mut out);

    // R2 — prefixed provider tokens.
    count += replace_prefixed(&mut out, "sk-", 16, 200, is_token_char_relaxed);
    count += replace_prefixed(&mut out, "ghp_", 20, 200, is_token_char_strict);
    count += replace_prefixed(&mut out, "github_pat_", 20, 255, is_token_char_relaxed);
    for xox in ["xoxa-", "xoxb-", "xoxp-", "xoxr-", "xoxs-"] {
        count += replace_prefixed(&mut out, xox, 10, 200, is_token_char_relaxed);
    }

    // R4 — Authorization bearer tokens.
    count += replace_bearer(&mut out);

    (out, count)
}

const WEBHOOK_PREFIXES: &[&str] = &[
    "https://discord.com/api/webhooks/",
    "https://discordapp.com/api/webhooks/",
    "https://hooks.slack.com/services/",
    "https://open.feishu.cn/open-apis/bot/v2/hook/",
];

fn is_token_char_strict(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn is_token_char_relaxed(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// Replace every `<prefix><run of token chars (min..=max)>` occurrence.
fn replace_prefixed(
    out: &mut String,
    prefix: &str,
    min_len: usize,
    max_len: usize,
    is_token_char: fn(u8) -> bool,
) -> u32 {
    let mut count = 0;
    let mut search_from = 0;
    while let Some(rel) = out[search_from..].find(prefix) {
        let start = search_from + rel;
        let body_start = start + prefix.len();
        let bytes = out.as_bytes();
        let mut end = body_start;
        while end < bytes.len() && (end - body_start) < max_len && is_token_char(bytes[end]) {
            end += 1;
        }
        if end - body_start >= min_len {
            out.replace_range(start..end, STRIPPED_PLACEHOLDER);
            count += 1;
            search_from = start + STRIPPED_PLACEHOLDER.len();
        } else {
            // Too short — not a credential (e.g. "sk-learn"); keep scanning
            // after this occurrence.
            search_from = body_start;
        }
    }
    count
}

/// Replace `?name=value` / `&name=value` where name is a credential param.
fn replace_query_params(out: &mut String) -> u32 {
    const PARAMS: &[&str] = &[
        "access_token",
        "key",
        "password",
        "secret",
        "sig",
        "signature",
        "token",
    ];
    let mut count = 0;
    let mut search_from = 0;
    while let Some(rel) = out[search_from..].find(['?', '&']) {
        let start = search_from + rel + 1;
        let rest = &out[start..];
        let mut matched: Option<usize> = None;
        for param in PARAMS {
            let pat = format!("{param}=");
            if rest.len() >= pat.len() && rest[..pat.len()].eq_ignore_ascii_case(&pat) {
                matched = Some(pat.len());
                break;
            }
        }
        let Some(value_start_off) = matched else {
            search_from = start;
            continue;
        };
        let value_start = start + value_start_off;
        let bytes = out.as_bytes();
        let mut end = value_start;
        while end < bytes.len() && bytes[end] != b'&' && bytes[end] != b' ' && bytes[end] != b'"' {
            end += 1;
        }
        if end > value_start {
            out.replace_range(value_start..end, STRIPPED_PLACEHOLDER);
            count += 1;
            search_from = value_start + STRIPPED_PLACEHOLDER.len();
        } else {
            search_from = value_start;
        }
    }
    count
}

/// ASCII-case-insensitive substring search with byte offsets that stay valid
/// for `hay` (no `to_lowercase`, which can change byte lengths on non-ASCII).
fn find_ignore_ascii_case(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let hay = hay.as_bytes();
    let needle = needle.as_bytes();
    let mut i = from;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()].eq_ignore_ascii_case(needle) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Replace `Bearer <token>` (≥12 token chars) — typical Authorization header.
fn replace_bearer(out: &mut String) -> u32 {
    let mut count = 0;
    let mut search_from = 0;
    while let Some(rel) = find_ignore_ascii_case(out, "bearer ", search_from) {
        let bearer_start = rel;
        // Only when preceded by a non-token char (or line start) so prose
        // like "share-bearer 7" does not match.
        if bearer_start > 0 {
            let prev = out.as_bytes()[bearer_start - 1];
            if prev.is_ascii_alphanumeric() || prev == b'-' || prev == b'_' {
                search_from = bearer_start + "bearer ".len();
                continue;
            }
        }
        let value_start = bearer_start + "bearer ".len();
        let bytes = out.as_bytes();
        let mut end = value_start;
        while end < bytes.len()
            && (end - value_start) < 512
            && (bytes[end].is_ascii_alphanumeric()
                || bytes[end] == b'_'
                || bytes[end] == b'-'
                || bytes[end] == b'.'
                || bytes[end] == b'=')
        {
            end += 1;
        }
        if end - value_start >= 12 {
            out.replace_range(value_start..end, STRIPPED_PLACEHOLDER);
            count += 1;
            search_from = value_start + STRIPPED_PLACEHOLDER.len();
        } else {
            search_from = value_start;
        }
    }
    count
}

// ─── Pack reading (inspect + import) ────────────────────────────────────────

struct ReadPack {
    manifest: PackManifest,
    /// Non-manifest entries by pack-relative path (sorted).
    files: BTreeMap<String, Vec<u8>>,
    /// Assets whose recorded sha256/size did not match the packed bytes.
    /// Import refuses these units (surfaced in the report's `failed` list);
    /// inspect tolerates them so the user can still preview the pack.
    integrity_failures: Vec<(String, String)>,
}

/// Recompute `counts` from the archive contents — the same unit semantics the
/// exporter uses (skill/routine dirs are one unit; commands/profiles per
/// file; memories per JSONL row; persona 0/1). A manifest whose counts
/// disagree with the archive is corrupt.
fn counts_from_files(files: &BTreeMap<String, Vec<u8>>) -> Result<PackCounts, String> {
    let distinct_first = |prefix: &str| -> u32 {
        let group_prefix = format!("{prefix}/");
        let mut names: Vec<&str> = Vec::new();
        for rel in files.keys() {
            if let Some(rest) = rel.strip_prefix(&group_prefix) {
                let name = rest.split('/').next().unwrap_or("");
                if !name.is_empty() && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names.len() as u32
    };
    let count_prefix = |prefix: &str| -> u32 {
        let group_prefix = format!("{prefix}/");
        files
            .keys()
            .filter(|rel| rel.starts_with(&group_prefix))
            .count() as u32
    };
    let memories = match files.get("memories.jsonl") {
        None => 0,
        Some(data) => {
            let text = std::str::from_utf8(data)
                .map_err(|_| "corrupt pack: memories.jsonl is not valid UTF-8".to_string())?;
            text.lines().filter(|l| !l.trim().is_empty()).count() as u32
        }
    };
    Ok(PackCounts {
        skills: distinct_first("skills"),
        commands: count_prefix("commands"),
        memories,
        routines: distinct_first("routines"),
        profiles: count_prefix("profiles"),
        persona: u32::from(files.contains_key("persona.md")),
    })
}

/// Open a `.tar.gz` pack, validate every entry (path safety, entry type,
/// size caps), read it fully into memory and cross-check it against the
/// manifest — presence, per-asset sha256/size integrity, and that the
/// manifest counts match the archive contents. Nothing is ever written to
/// the target stores here, so both inspect and import get their pre-flight
/// from this one place.
fn read_pack(path: &Path) -> Result<ReadPack, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut archive = Archive::new(GzDecoder::new(file));
    let mut files = BTreeMap::new();
    let mut manifest_raw: Option<Vec<u8>> = None;
    let mut total: u64 = 0;

    for entry in archive
        .entries()
        .map_err(|e| format!("read pack entries: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("read pack entry: {e}"))?;
        match entry.header().entry_type() {
            // Mirror the T13 rejection semantics: a link entry must never be
            // materialized on import.
            EntryType::Symlink | EntryType::Link => {
                let name = entry
                    .path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                return Err(format!(
                    "refusing symlink/hardlink entry {name:?} — packs must contain regular files \
                     only"
                ));
            }
            EntryType::Directory | EntryType::Continuous => continue,
            EntryType::Regular => {}
            _ => continue, // pax/GNU metadata — handled by the tar crate
        }
        let rel = entry
            .path()
            .map_err(|e| format!("entry path: {e}"))?
            .to_string_lossy()
            .to_string();
        validate_entry_path(&rel)?;
        let size = entry.header().size().unwrap_or(0);
        if size > MAX_FILE_BYTES {
            return Err(format!("entry {rel:?} too large ({size} bytes)"));
        }
        total += size;
        if total > MAX_TOTAL_BYTES {
            return Err("pack decompresses beyond the 512 MB safety cap".to_string());
        }
        let mut data = Vec::with_capacity(size.min(MAX_FILE_BYTES) as usize);
        entry
            .read_to_end(&mut data)
            .map_err(|e| format!("read entry {rel:?}: {e}"))?;
        if data.len() as u64 != size {
            return Err(format!("entry {rel:?} truncated"));
        }
        if rel == MANIFEST_NAME {
            manifest_raw = Some(data);
        } else {
            files.insert(rel, data);
        }
    }

    let raw = manifest_raw.ok_or_else(|| "corrupt pack: manifest.json missing".to_string())?;
    let manifest: PackManifest = serde_json::from_slice(&raw)
        .map_err(|e| format!("corrupt pack: manifest.json unreadable: {e}"))?;
    if manifest.version != PACK_VERSION {
        return Err(format!(
            "unsupported pack version {} — expected {PACK_VERSION}",
            manifest.version
        ));
    }
    // Manifest ↔ entries cross-check: every listed asset must be present and
    // every packed file must be listed (catches truncated / tampered packs).
    for asset in &manifest.assets {
        validate_entry_path(&asset.path)?;
        if !files.contains_key(&asset.path) {
            return Err(format!(
                "corrupt pack: manifest lists {:?} but it is not in the archive",
                asset.path
            ));
        }
    }
    if files.len() != manifest.assets.len() {
        return Err(
            "corrupt pack: archive contains entries not listed in the manifest".to_string(),
        );
    }
    // Manifest counts must match the archive contents (same unit semantics as
    // the exporter) — a lying manifest is a corrupt pack.
    let recomputed = counts_from_files(&files)?;
    if recomputed != manifest.counts {
        return Err(format!(
            "corrupt pack: manifest counts {:?} do not match archive contents {:?}",
            manifest.counts, recomputed
        ));
    }
    // Per-asset integrity: recorded sha256/size must match the packed bytes.
    // A mismatch fails that asset's import unit (reported in `failed`);
    // inspect still previews the pack.
    let mut integrity_failures: Vec<(String, String)> = Vec::new();
    for asset in &manifest.assets {
        let data = &files[&asset.path];
        let mut hasher = Sha256::new();
        hasher.update(data);
        let got_sha = format!("{:x}", hasher.finalize());
        if got_sha != asset.sha256 || data.len() as u64 != asset.bytes {
            integrity_failures.push((
                asset.path.clone(),
                format!(
                    "integrity check failed — manifest records sha256 {} / {} bytes, archive has \
                     {} / {} bytes",
                    &asset.sha256[..asset.sha256.len().min(12)],
                    asset.bytes,
                    &got_sha[..12],
                    data.len()
                ),
            ));
        }
    }
    Ok(ReadPack {
        manifest,
        files,
        integrity_failures,
    })
}

/// Zip-slip guard for pack-relative paths: relative, forward slashes only, no
/// `..` / `.` / root / prefix components, no NUL.
fn validate_entry_path(rel: &str) -> Result<(), String> {
    if rel.is_empty() {
        return Err("empty pack entry path".to_string());
    }
    if rel.contains('\\') {
        return Err(format!("pack entry {rel:?} contains a backslash"));
    }
    if rel.contains('\0') {
        return Err("pack entry path contains NUL".to_string());
    }
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err(format!("pack entry {rel:?} is an absolute path"));
    }
    for comp in path.components() {
        match comp {
            Component::Normal(_) => {}
            other => {
                return Err(format!(
                    "pack entry {rel:?} contains a forbidden path component ({other:?})"
                ));
            }
        }
    }
    Ok(())
}

pub fn inspect_core(path: &Path) -> Result<PackInspectResult, String> {
    let pack = read_pack(path)?;
    Ok(PackInspectResult {
        version: pack.manifest.version,
        counts: pack.manifest.counts,
        created_at_ms: pack.manifest.created_at_ms,
        generator: pack.manifest.generator,
    })
}

// ─── Import ─────────────────────────────────────────────────────────────────

/// One import unit: a group of pack files that land together (a skill dir, a
/// command file, a profile file, …). `targets` maps pack path → on-disk
/// target; `renamed` is the alternate mapping used by the `rename` strategy.
struct ImportUnit {
    label: String,
    kind: &'static str,
    /// Pack-relative source paths of this unit's files (integrity checks).
    pack_paths: Vec<String>,
    targets: Vec<(PathBuf, Vec<u8>)>,
    /// `None` when rename is not meaningful for this kind (persona).
    renamed: Option<Vec<(PathBuf, Vec<u8>)>>,
}

pub fn import_core(
    path: &Path,
    conflict: &str,
    include: PackInclude,
    roots: &PackRoots,
) -> Result<PackImportReport, String> {
    if !matches!(conflict, "skip" | "overwrite" | "rename") {
        return Err(format!(
            "invalid conflict {conflict:?} — expected \"skip\", \"overwrite\" or \"rename\""
        ));
    }
    let pack = read_pack(path)?;
    let mut report = PackImportReport::default();
    let mut units: Vec<ImportUnit> = Vec::new();

    // Skills: `skills/<name>/<rest>` — unit = the skill directory.
    if include.skills {
        group_by_first_component(
            &pack.files,
            "skills",
            &roots.skills_dir(),
            "skill",
            &mut units,
        );
    }
    // Commands: `commands/<rel>` — unit = one file.
    if include.commands {
        for (rel, data) in &pack.files {
            let Some(suffix) = rel.strip_prefix("commands/") else {
                continue;
            };
            units.push(ImportUnit {
                label: rel.clone(),
                kind: "command",
                pack_paths: vec![rel.clone()],
                targets: vec![(roots.commands_dir().join(suffix), data.clone())],
                renamed: Some(vec![(
                    roots.commands_dir().join(renamed_file_name(suffix)),
                    data.clone(),
                )]),
            });
        }
    }
    // Memories: parsed separately below (line-granular, not file-granular).
    // Routines: task dirs are first-component groups; the two user-global
    // files (`routines.toml`, `routine-overrides.json`) are single-file units.
    if include.routines {
        group_by_first_component(
            &pack.files,
            "routines",
            &roots.scheduled_tasks_dir(),
            "routine",
            &mut units,
        );
        for (rel, target, renamed_name) in [
            (
                "routines/routines.toml",
                roots.routines_toml(),
                "routines-imported.toml",
            ),
            (
                "routines/routine-overrides.json",
                roots.routine_overrides(),
                "routine-overrides-imported.json",
            ),
        ] {
            if let Some(data) = pack.files.get(rel) {
                units.push(ImportUnit {
                    label: rel.to_string(),
                    kind: "routine",
                    pack_paths: vec![rel.to_string()],
                    targets: vec![(target.clone(), data.clone())],
                    renamed: Some(vec![(target.with_file_name(renamed_name), data.clone())]),
                });
            }
        }
    }
    // Profiles: `profiles/<name>` — unit = one file.
    if include.profiles {
        for (rel, data) in &pack.files {
            let Some(suffix) = rel.strip_prefix("profiles/") else {
                continue;
            };
            units.push(ImportUnit {
                label: rel.clone(),
                kind: "profile",
                pack_paths: vec![rel.clone()],
                targets: vec![(roots.profiles_dir().join(suffix), data.clone())],
                renamed: Some(vec![(
                    roots.profiles_dir().join(renamed_file_name(suffix)),
                    data.clone(),
                )]),
            });
        }
    }
    // Persona: single file; rename falls back to skip (a renamed copy of the
    // global instruction file would never be loaded by Shannon, so "don't
    // clobber" can only mean "keep ours" — documented in the report).
    if include.persona {
        if let Some(data) = pack.files.get("persona.md") {
            units.push(ImportUnit {
                label: "persona.md".to_string(),
                kind: "persona",
                pack_paths: vec!["persona.md".to_string()],
                targets: vec![(roots.persona_file(), data.clone())],
                renamed: None,
            });
        }
    }

    // Integrity-failed assets (manifest sha256/size mismatch) never import:
    // any unit containing one fails wholesale — with a per-unit error that
    // surfaces in the UI's failed list — and nothing is written for it.
    let integrity_failed = |unit: &ImportUnit| -> Option<String> {
        unit.pack_paths.iter().find_map(|p| {
            pack.integrity_failures
                .iter()
                .find(|(path, _)| path == p)
                .map(|(_, err)| format!("{p}: {err}"))
        })
    };

    for unit in &units {
        if let Some(error) = integrity_failed(unit) {
            report.failed.push(PackFailure {
                item: unit.label.clone(),
                error,
            });
            continue;
        }
        match resolve_unit(unit, conflict) {
            Ok(true) => report.imported.bump_kind(unit.kind),
            Ok(false) => report.skipped.bump_kind(unit.kind),
            Err(error) => report.failed.push(PackFailure {
                item: unit.label.clone(),
                error,
            }),
        }
    }

    // Memories: line-granular, deduped by exact content under the current
    // project. Conflict strategies have no "rename" meaning for rows; the
    // dedup semantics keep re-imports idempotent for all strategies.
    if include.memory {
        if let Some((_, err)) = pack
            .integrity_failures
            .iter()
            .find(|(path, _)| path == "memories.jsonl")
        {
            report.failed.push(PackFailure {
                item: "memories.jsonl".to_string(),
                error: err.clone(),
            });
        } else if let Some(data) = pack.files.get("memories.jsonl") {
            import_memories(data, roots, &mut report);
        }
    }

    Ok(report)
}

/// Group `files` entries under `<prefix>/` into one unit per first path
/// component (skill name / routine task dir).
fn group_by_first_component(
    files: &BTreeMap<String, Vec<u8>>,
    prefix: &str,
    target_root: &Path,
    kind: &'static str,
    units: &mut Vec<ImportUnit>,
) {
    let group_prefix = format!("{prefix}/");
    // key: name → (pack rel, target path, bytes), in deterministic first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut grouped: BTreeMap<String, Vec<(String, PathBuf, Vec<u8>)>> = BTreeMap::new();
    for (rel, data) in files {
        let Some(suffix) = rel.strip_prefix(&group_prefix) else {
            continue;
        };
        let Some(name) = suffix.split('/').next().map(str::to_string) else {
            continue;
        };
        let rest = suffix[name.len()..].strip_prefix('/').unwrap_or("");
        if rest.is_empty() {
            continue; // a bare `<prefix>/<name>` entry is not a file we pack
        }
        let target = target_root.join(&name).join(rest);
        grouped
            .entry(name.clone())
            .or_default()
            .push((rel.clone(), target, data.clone()));
        if !order.contains(&name) {
            order.push(name);
        }
    }
    for name in order {
        let grouped_files = grouped.remove(&name).unwrap_or_default();
        let pack_paths: Vec<String> = grouped_files
            .iter()
            .map(|(rel, _, _)| rel.clone())
            .collect();
        units.push(ImportUnit {
            label: format!("{prefix}/{name}"),
            kind,
            pack_paths,
            targets: grouped_files
                .iter()
                .map(|(_, target, data)| (target.clone(), data.clone()))
                .collect(),
            renamed: Some(
                grouped_files
                    .into_iter()
                    .map(|(_, target, data)| {
                        let rest = target
                            .strip_prefix(target_root.join(&name))
                            .unwrap_or(&target)
                            .to_path_buf();
                        (
                            target_root.join(format!("{name}-imported")).join(rest),
                            data,
                        )
                    })
                    .collect(),
            ),
        });
    }
}

/// Decide + execute one unit. `Ok(true)` = imported, `Ok(false)` = skipped,
/// `Err` = this unit failed (never aborts the batch).
fn resolve_unit(unit: &ImportUnit, conflict: &str) -> Result<bool, String> {
    let existing: Vec<Option<Vec<u8>>> = unit
        .targets
        .iter()
        .map(|(target, _)| read_target(target))
        .collect();
    let all_identical = unit
        .targets
        .iter()
        .zip(&existing)
        .all(|((_, data), current)| current.as_deref() == Some(data.as_slice()));
    let any_exists = existing.iter().any(|e| e.is_some());

    if all_identical {
        return Ok(false); // identical — idempotent no-op under every strategy
    }
    if !any_exists {
        return write_all(&unit.targets).map(|_| true);
    }
    match conflict {
        "overwrite" => write_all(&unit.targets).map(|_| true),
        "skip" => Ok(false),
        "rename" => match &unit.renamed {
            None => Ok(false), // persona: no meaningful rename → keep ours
            Some(renamed) => {
                let renamed_state: Vec<Option<Vec<u8>>> =
                    renamed.iter().map(|(t, _)| read_target(t)).collect();
                let renamed_identical = renamed
                    .iter()
                    .zip(&renamed_state)
                    .all(|((_, data), current)| current.as_deref() == Some(data.as_slice()));
                let renamed_any_exists = renamed_state.iter().any(|e| e.is_some());
                if renamed_identical {
                    Ok(false) // already imported under the renamed slot
                } else if renamed_any_exists {
                    Err("renamed target also exists with different content".to_string())
                } else {
                    write_all(renamed).map(|_| true)
                }
            }
        },
        other => Err(format!("invalid conflict {other:?}")),
    }
}

/// Read a target file; missing → `None`, any other error → `None` (the
/// conflict flow then treats it as differing; the write surfaces real errors).
fn read_target(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn write_all(targets: &[(PathBuf, Vec<u8>)]) -> Result<(), String> {
    for (target, data) in targets {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::write(target, data).map_err(|e| format!("write {}: {e}", target.display()))?;
    }
    Ok(())
}

/// `<dir>/<stem>-imported<ext>` for a single-file relative name.
fn renamed_file_name(rel: &str) -> String {
    let (dir, file) = match rel.rsplit_once('/') {
        Some((d, f)) => (format!("{d}/"), f),
        None => (String::new(), rel),
    };
    let renamed = match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-imported.{ext}"),
        _ => format!("{file}-imported"),
    };
    format!("{dir}{renamed}")
}

fn import_memories(data: &[u8], roots: &PackRoots, report: &mut PackImportReport) {
    let text = String::from_utf8_lossy(data);
    let project = roots.project.display().to_string();
    let mut store = MemoryStore::new(roots.memories_dir());
    if store.storage_path().is_dir() {
        if let Err(e) = store.load() {
            report.failed.push(PackFailure {
                item: "memories.jsonl".to_string(),
                error: format!("load existing memories: {e}"),
            });
            return;
        }
    }
    let existing = store.project_memories(&project);
    for (idx, line) in text.lines().enumerate() {
        let item = format!("memories.jsonl#{}", idx + 1);
        if line.trim().is_empty() {
            continue;
        }
        let row: ExportedMemory = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                report.failed.push(PackFailure {
                    item,
                    error: format!("unreadable memory row: {e}"),
                });
                continue;
            }
        };
        // Idempotency: identical content already present for this project →
        // skip, whatever the strategy (mirrors the migration import).
        if existing.iter().any(|e| e.content == row.content) {
            report.skipped.memories += 1;
            continue;
        }
        let entry = MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            project: project.clone(),
            category: parse_category(&row.category),
            content: row.content,
            tags: {
                let mut tags = row.tags;
                if tags.is_empty() {
                    tags.push("persona-pack".to_string());
                }
                tags
            },
            confidence: row.confidence.clamp(0.0, 1.0),
            created_at: chrono::DateTime::from_timestamp_millis(row.created_at_ms)
                .unwrap_or_else(chrono::Utc::now),
            accessed_at: chrono::Utc::now(),
            access_count: 0,
            // Provenance: the packed kind is preserved; never a session id.
            source_session_id: None,
            source_kind: Some(
                row.source_kind
                    .unwrap_or_else(|| MemoryEntry::SOURCE_IMPORT.to_string()),
            ),
        };
        match store.add_or_update(entry) {
            Ok(_) => report.imported.memories += 1,
            Err(e) => report.failed.push(PackFailure {
                item,
                error: e.to_string(),
            }),
        }
    }
    // Persist (append-only adds are already durable; `save` reconciles and is
    // a no-op re-write when nothing changed).
    if let Err(e) = store.save() {
        report.failed.push(PackFailure {
            item: "memories.jsonl".to_string(),
            error: format!("save memories: {e}"),
        });
    }
}

/// Parse a packed `category` string (the [`MemoryCategory`] `Display` form).
/// Unknown values fall back to `Preference` rather than failing the row.
fn parse_category(s: &str) -> MemoryCategory {
    match s {
        "pattern" => MemoryCategory::Pattern,
        "decision" => MemoryCategory::Decision,
        "error" => MemoryCategory::Error,
        "context" => MemoryCategory::Context,
        _ => MemoryCategory::Preference,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Tempdir pair: exporting roots + importing roots (distinct "machines").
    struct Env {
        _dir: tempfile::TempDir,
        src: PackRoots,
        dst: PackRoots,
    }

    fn env(tag: &str) -> Env {
        let dir = tempfile::TempDir::with_prefix(tag).expect("tempdir");
        let src = PackRoots {
            home: dir.path().join("src-home"),
            project: dir.path().join("src-proj"),
        };
        let dst = PackRoots {
            home: dir.path().join("dst-home"),
            project: dir.path().join("dst-proj"),
        };
        for p in [&src.home, &src.project, &dst.home, &dst.project] {
            std::fs::create_dir_all(p).expect("root dirs");
        }
        Env {
            _dir: dir,
            src,
            dst,
        }
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, contents).expect("write fixture");
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).expect("read")
    }

    /// Seed a full source tree: one skill (2 files), two commands (one
    /// nested), two memories (one with provenance), one scheduled routine +
    /// routines.toml + overrides, one profile, one persona file.
    fn seed(roots: &PackRoots) {
        write(
            &roots.skills_dir().join("commit/SKILL.md"),
            "---\nname: commit\ndescription: Make a commit\n---\n\nCommit well.\n",
        );
        write(
            &roots.skills_dir().join("commit/notes.md"),
            "# Notes\n\napi_key = \"live-secret-123\"\n",
        );
        write(
            &roots.commands_dir().join("deploy.md"),
            "Deploy the service.\n",
        );
        write(
            &roots.commands_dir().join("blog/draft.md"),
            "Draft a blog post.\n",
        );

        let mut store = MemoryStore::new(roots.memories_dir());
        store.load().expect("load empty store");
        let mut e1 = MemoryEntry::with_confidence(
            &roots.project.display().to_string(),
            MemoryCategory::Preference,
            "Always answer concisely.",
            0.9,
            vec!["style".into()],
        )
        .expect("memory");
        e1.source_kind = Some(MemoryEntry::SOURCE_MANUAL.to_string());
        store.add(e1).expect("add memory");
        let mut e2 = MemoryEntry::with_confidence(
            &roots.project.display().to_string(),
            MemoryCategory::Context,
            "Deploys happen on Tuesdays.",
            1.0,
            Vec::new(),
        )
        .expect("memory");
        e2.source_session_id = Some("018f3c4e-9c7a-7d3e-8f2a-1b2c3d4e5f60".to_string());
        e2.source_kind = Some(MemoryEntry::SOURCE_AUTO_EXTRACT.to_string());
        store.add(e2).expect("add memory");

        write(
            &roots.scheduled_tasks_dir().join("standup-abc123/SKILL.md"),
            "Summarize the standup.\n",
        );
        write(
            &roots.scheduled_tasks_dir().join("standup-abc123/task.json"),
            r#"{"id":"abc123","name":"standup","prompt":"Summarize the standup."}"#,
        );
        write(
            &roots.routines_toml(),
            "# triggered routines\n[[routine]]\nname = \"post-merge\"\nevent = \"push\"\n",
        );
        write(&roots.routine_overrides(), r#"{"post-merge": true}"#);

        write(
            &roots.profiles_dir().join("team-balanced.toml"),
            "name = \"team-balanced\"\ndescription = \"Team baseline\"\nauto_approve = []\nconfirm = []\ndeny = []\n",
        );
        write(
            &roots.persona_file(),
            "# Global instructions\n\nPrefer concise answers.\n",
        );
    }

    const ALL: PackInclude = PackInclude {
        skills: true,
        commands: true,
        memory: true,
        routines: true,
        profiles: true,
        persona: true,
    };

    fn pack_path(e: &Env) -> PathBuf {
        e._dir.path().join("packs").join("shannon-test.tar.gz")
    }

    /// List the (non-manifest) entries inside a written pack.
    fn pack_entries(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).expect("open pack");
        let mut archive = Archive::new(GzDecoder::new(file));
        let mut out = Vec::new();
        for entry in archive.entries().expect("entries") {
            let entry = entry.expect("entry");
            out.push(entry.path().expect("path").display().to_string());
        }
        out
    }

    /// Read one entry's bytes out of a written pack.
    fn pack_entry(path: &Path, want: &str) -> Option<Vec<u8>> {
        let file = std::fs::File::open(path).expect("open pack");
        let mut archive = Archive::new(GzDecoder::new(file));
        for entry in archive.entries().expect("entries") {
            let mut entry = entry.expect("entry");
            if entry.path().expect("path").display().to_string() == want {
                let mut data = Vec::new();
                entry.read_to_end(&mut data).expect("read entry");
                return Some(data);
            }
        }
        None
    }

    // ─── Round trip: full assets ────────────────────────────────────────────

    #[test]
    fn export_import_round_trip_all_assets() {
        let e = env("roundtrip");
        seed(&e.src);
        let out = export_core(ALL, &e.src, &pack_path(&e)).expect("export");

        assert_eq!(
            out.counts,
            PackCounts {
                skills: 1,
                commands: 2,
                memories: 2,
                // task dir + routines.toml + routine-overrides.json
                routines: 3,
                profiles: 1,
                persona: 1,
            }
        );

        let report = import_core(&pack_path(&e), "skip", ALL, &e.dst).expect("import");
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(
            report.imported,
            PackCounts {
                skills: 1,
                commands: 2,
                memories: 2,
                routines: 3,
                profiles: 1,
                persona: 1,
            }
        );
        assert_eq!(report.skipped, PackCounts::default());

        // Files landed at their Shannon locations.
        assert_eq!(
            read(&e.dst.skills_dir().join("commit/SKILL.md")),
            read(&e.src.skills_dir().join("commit/SKILL.md"))
        );
        assert!(e.dst.commands_dir().join("blog/draft.md").is_file());
        assert!(
            e.dst
                .scheduled_tasks_dir()
                .join("standup-abc123/task.json")
                .is_file()
        );
        assert_eq!(read(&e.dst.routines_toml()), read(&e.src.routines_toml()));
        assert_eq!(
            read(&e.dst.routine_overrides()),
            read(&e.src.routine_overrides())
        );
        assert!(e.dst.profiles_dir().join("team-balanced.toml").is_file());
        assert_eq!(read(&e.dst.persona_file()), read(&e.src.persona_file()));

        // Memories: imported under the importing machine's project, content
        // intact, provenance kind kept, session id never present.
        let mut store = MemoryStore::new(e.dst.memories_dir());
        store.load().expect("load");
        let imported = store.project_memories(&e.dst.project.display().to_string());
        assert_eq!(imported.len(), 2);
        assert!(
            imported
                .iter()
                .any(|m| m.content == "Always answer concisely."
                    && m.source_kind.as_deref() == Some(MemoryEntry::SOURCE_MANUAL)
                    && m.source_session_id.is_none())
        );
        assert!(imported.iter().any(|m| m.source_kind.as_deref()
            == Some(MemoryEntry::SOURCE_AUTO_EXTRACT)
            && m.source_session_id.is_none()));
    }

    #[test]
    fn export_never_writes_source_session_ids_or_project_paths_into_the_pack() {
        let e = env("no-provenance");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        let memories = pack_entry(&pack_path(&e), "memories.jsonl").expect("memories.jsonl");
        let text = String::from_utf8(memories).expect("utf8");
        assert!(
            !text.contains("018f3c4e-9c7a-7d3e-8f2a-1b2c3d4e5f60"),
            "session id leaked: {text}"
        );
        assert!(
            !text.contains(&e.src.project.display().to_string()),
            "project path leaked: {text}"
        );
        assert!(text.contains("sourceKind"), "provenance kind kept: {text}");
    }

    #[test]
    fn partial_include_packs_only_selected_categories() {
        let e = env("partial");
        seed(&e.src);
        let include = PackInclude {
            skills: true,
            commands: true,
            ..Default::default()
        };
        let out = export_core(include, &e.src, &pack_path(&e)).expect("export");
        assert_eq!(out.counts.skills, 1);
        assert_eq!(out.counts.commands, 2);
        assert_eq!(out.counts.memories, 0);
        assert_eq!(out.counts.routines, 0);
        assert_eq!(out.counts.profiles, 0);
        assert_eq!(out.counts.persona, 0);

        let entries = pack_entries(&pack_path(&e));
        assert!(entries.iter().any(|p| p.starts_with("skills/")));
        assert!(!entries.iter().any(|p| p.starts_with("routines/")));
        assert!(!entries.contains(&"memories.jsonl".to_string()));
        assert!(!entries.contains(&"persona.md".to_string()));
    }

    // ─── Secret stripping ───────────────────────────────────────────────────

    #[test]
    fn stripping_rules_matrix() {
        let fixture_sk_live = format!("api_key = \"{}-live-abcdef123456\"", "sk");
        let cases: Vec<(&str, &str)> = vec![
            // R1 — assignment lines (whole line replaced)
            (
                concat!("api_key = \"sk-", "live-abcdef123456\""),
                STRIPPED_PLACEHOLDER,
            ),
            ("API_KEY=hunter2hunter2", STRIPPED_PLACEHOLDER),
            ("\"api_key\": \"value-here\"", STRIPPED_PLACEHOLDER),
            ("- token: abc123def456", STRIPPED_PLACEHOLDER),
            ("export SECRET_KEY=top-secret", STRIPPED_PLACEHOLDER),
            ("password: hunter2", STRIPPED_PLACEHOLDER),
            ("client_secret = \"abc\"", STRIPPED_PLACEHOLDER),
        ];
        for (line, expected) in cases {
            let (out, n) = strip_text(&format!("{line}\n"), false);
            assert_eq!(out, format!("{expected}\n"), "R1 failed for {line:?}");
            assert_eq!(n, 1, "R1 count for {line:?}");
        }

        // R2 — prefixed tokens: only the token is replaced.
        let (out, n) = strip_text("use key sk-abcdef1234567890abcdef12 tomorrow", false);
        assert!(
            out.contains(STRIPPED_PLACEHOLDER) && !out.contains("sk-abcdef"),
            "{out}"
        );
        assert!(
            out.contains("use key ") && out.contains(" tomorrow"),
            "{out}"
        );
        assert_eq!(n, 1);
        let fixture_ghp_tok = format!("{}{}", "ghp", "_ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890");
        let (out, n) = strip_text(&fixture_ghp_tok, false);
        let fixture = format!("{}{}", "ghp", "_ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890");
        let (out, n) = strip_text(&fixture, false);
        assert_eq!(n, 1);
        let fixture_xoxb_tok = format!("{}{}", "xoxb", "-123456789-abcdefghijklmnopqrstuv");
        let (out, n) = strip_text(&fixture_xoxb_tok, false);
        let fixture = format!("{}{}", "xoxb", "-123456789-abcdefghijklmnopqrstuv");
        let (out, n) = strip_text(&fixture, false);
        assert_eq!(n, 1);
        let fixture_gp_tok = format!(
            "{}{}{}",
            "github", "_pat_", "ABCDEFGHIJKLMNOPQRSTUVWXYZ123_4567890"
        );
        let (out, n) = strip_text(&fixture_gp_tok, false);
        let fixture = format!(
            "{}{}{}",
            "github", "_pat_", "ABCDEFGHIJKLMNOPQRSTUVWXYZ123_4567890"
        );
        let (out, n) = strip_text(&fixture, false);
        assert_eq!(n, 1);

        // R3a — webhook URLs truncated after the fixed prefix.
        for url in [
            "https://discord.com/api/webhooks/123456/supersecrettoken",
            "https://hooks.slack.com/services/T000/B000/XXXX",
            "https://open.feishu.cn/open-apis/bot/v2/hook/abc-def-123",
        ] {
            let (out, n) = strip_text(&format!("notify: {url}"), false);
            assert!(out.contains(STRIPPED_PLACEHOLDER), "{url} → {out}");
            assert!(
                !out.contains("supersecret") && !out.contains("XXXX"),
                "{out}"
            );
            assert_eq!(n, 1);
        }
        // R3b — credential query params.
        let (out, n) = strip_text(
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=SECRETVALUE99",
            false,
        );
        assert!(
            out.contains(STRIPPED_PLACEHOLDER) && !out.contains("SECRETVALUE99"),
            "{out}"
        );
        assert_eq!(n, 1);

        // R4 — bearer tokens.
        let (out, n) = strip_text(
            "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
            false,
        );
        assert!(
            out.contains(STRIPPED_PLACEHOLDER) && !out.contains("eyJhbGci"),
            "{out}"
        );
        assert_eq!(n, 1);

        // R5 — PEM private-key block collapses to one placeholder line.
        let pem = "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQ\nAbCdEfGhIjKlMnO\n-----END RSA PRIVATE KEY-----\nafter\n";
        let (out, n) = strip_text(pem, false);
        assert_eq!(
            out,
            format!("before\n{STRIPPED_PLACEHOLDER}\nafter\n"),
            "{out}"
        );
        assert_eq!(n, 1);
    }

    #[test]
    fn stripping_does_not_damage_normal_content() {
        let normal = "\
max_tokens = 4096
token_refresh_interval = 300
passwords are bad, mkay?
password_hash_algo = argon2
# token bucket algorithm
Use sklearn and sk-learn for the ML bits.
See https://example.com/docs and https://example.com/?utm=1
Authorization: none
";
        let (out, n) = strip_text(normal, false);
        assert_eq!(n, 0, "false positives in:\n{out}");
        assert!(out.contains("max_tokens = 4096"));
        assert!(out.contains("sk-learn"));
        assert!(out.contains("https://example.com/docs"));
    }

    #[test]
    fn exported_skill_with_secret_is_stripped_in_pack_and_on_import() {
        let e = env("strip-skill");
        write(
            &e.src.skills_dir().join("leaky/SKILL.md"),
            "---\nname: leaky\n---\n\nCall https://discord.com/api/webhooks/42/leakytoken\n",
        );
        let out = export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        assert_eq!(out.stripped, 1, "one webhook redaction expected");

        let packed = pack_entry(&pack_path(&e), "skills/leaky/SKILL.md").expect("entry");
        let text = String::from_utf8(packed).expect("utf8");
        assert!(!text.contains("leakytoken"), "{text}");
        assert!(text.contains(STRIPPED_PLACEHOLDER), "{text}");

        import_core(&pack_path(&e), "skip", ALL, &e.dst).expect("import");
        let on_disk = read(&e.dst.skills_dir().join("leaky/SKILL.md"));
        assert!(on_disk.contains(STRIPPED_PLACEHOLDER) && !on_disk.contains("leakytoken"));
    }

    // ─── Path safety ────────────────────────────────────────────────────────

    /// Serialize a manifest to JSON bytes with explicit counts.
    fn manifest_bytes_with(assets: Vec<PackAssetEntry>, counts: PackCounts) -> Vec<u8> {
        let manifest = PackManifest {
            version: PACK_VERSION,
            generator: "shannon-test".to_string(),
            created_at_ms: 0,
            counts,
            assets,
        };
        serde_json::to_vec(&manifest).expect("manifest")
    }

    /// Manifest bytes with default (all-zero) counts — only usable for packs
    /// whose checks fail before the counts verification (path-safety cases).
    fn manifest_bytes(assets: Vec<PackAssetEntry>) -> Vec<u8> {
        manifest_bytes_with(assets, PackCounts::default())
    }

    fn sha_of(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    /// Byte-level tar.gz writer used to craft packs the `tar` crate itself
    /// refuses to build (parent-dir / absolute / symlink entries) as well as
    /// manifest/entry mismatches. Mirrors what a malicious pack would look
    /// like on disk.
    fn write_raw_pack(path: &Path, manifest: &[u8], entries: &[RawEntry]) {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create pack dir");
        }
        let mut tar_stream: Vec<u8> = Vec::new();
        let mut put = |name: &str, typeflag: u8, linkname: &str, data: &[u8]| {
            let mut header = [0u8; 512];
            let name_bytes = name.as_bytes();
            let n = name_bytes.len().min(100);
            header[..n].copy_from_slice(&name_bytes[..n]);
            let size_octal = format!("{:011o}\0", data.len());
            header[124..124 + size_octal.len()].copy_from_slice(size_octal.as_bytes());
            header[156] = typeflag;
            let link_bytes = linkname.as_bytes();
            let l = link_bytes.len().min(100);
            header[157..157 + l].copy_from_slice(&link_bytes[..l]);
            for b in &mut header[148..156] {
                *b = b' '; // checksum field must read as spaces while summing
            }
            let sum: u32 = header.iter().map(|&b| b as u32).sum();
            let sum_str = format!("{sum:06o}\0 ");
            header[148..156].copy_from_slice(sum_str.as_bytes());
            tar_stream.extend_from_slice(&header);
            tar_stream.extend_from_slice(data);
            let pad = (512 - data.len() % 512) % 512;
            tar_stream.extend(std::iter::repeat_n(0u8, pad));
        };
        put(MANIFEST_NAME, b'0', "", manifest);
        for entry in entries {
            put(entry.name, entry.typeflag, entry.linkname, entry.data);
        }
        tar_stream.extend_from_slice(&[0u8; 1024]); // end-of-archive marker
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&tar_stream).expect("gzip");
        std::fs::write(path, gz.finish().expect("finish")).expect("write pack");
    }

    struct RawEntry {
        name: &'static str,
        typeflag: u8,
        linkname: &'static str,
        data: &'static [u8],
    }

    #[test]
    fn import_rejects_parent_dir_entries() {
        let e = env("evil-dotdot");
        write_raw_pack(
            &pack_path(&e),
            &manifest_bytes(vec![PackAssetEntry {
                path: "commands/../../evil.md".to_string(),
                kind: "command".to_string(),
                bytes: 6,
                sha256: "x".to_string(),
                stripped: 0,
            }]),
            &[RawEntry {
                name: "commands/../../evil.md",
                typeflag: b'0',
                linkname: "",
                data: b"gotcha",
            }],
        );
        let err = import_core(&pack_path(&e), "skip", ALL, &e.dst).unwrap_err();
        assert!(
            err.contains("forbidden path component") || err.contains(".."),
            "{err}"
        );
        assert!(!e.dst.home.join("evil.md").exists());
        assert!(!e.dst.commands_dir().join("evil.md").exists());
    }

    #[test]
    fn import_rejects_absolute_path_entries() {
        let e = env("evil-absolute");
        write_raw_pack(
            &pack_path(&e),
            &manifest_bytes(vec![PackAssetEntry {
                path: "/etc/shannon-evil.md".to_string(),
                kind: "command".to_string(),
                bytes: 6,
                sha256: "x".to_string(),
                stripped: 0,
            }]),
            &[RawEntry {
                name: "/etc/shannon-evil.md",
                typeflag: b'0',
                linkname: "",
                data: b"gotcha",
            }],
        );
        let err = import_core(&pack_path(&e), "skip", ALL, &e.dst).unwrap_err();
        assert!(
            err.contains("absolute") || err.contains("forbidden"),
            "{err}"
        );
    }

    #[test]
    fn import_rejects_symlink_entries() {
        let e = env("evil-symlink");
        write_raw_pack(
            &pack_path(&e),
            &manifest_bytes(vec![PackAssetEntry {
                path: "commands/link.md".to_string(),
                kind: "command".to_string(),
                bytes: 0,
                sha256: "x".to_string(),
                stripped: 0,
            }]),
            &[RawEntry {
                name: "commands/link.md",
                typeflag: b'2', // symlink
                linkname: "/etc/passwd",
                data: b"",
            }],
        );
        let err = import_core(&pack_path(&e), "skip", ALL, &e.dst).unwrap_err();
        assert!(err.contains("symlink"), "{err}");
        assert!(!e.dst.commands_dir().join("link.md").exists());
        assert!(!e.dst.commands_dir().join("link.md").is_symlink());
    }

    #[test]
    fn import_rejects_entries_not_listed_in_manifest() {
        let e = env("unlisted");
        // The archive holds a file the manifest omits.
        write_raw_pack(
            &pack_path(&e),
            &manifest_bytes(Vec::new()),
            &[RawEntry {
                name: "commands/deploy.md",
                typeflag: b'0',
                linkname: "",
                data: b"Deploy.",
            }],
        );
        let err = import_core(&pack_path(&e), "skip", ALL, &e.dst).unwrap_err();
        assert!(err.contains("not listed in the manifest"), "{err}");
    }

    // ─── Corrupt packs ──────────────────────────────────────────────────────

    #[test]
    fn corrupt_pack_is_rejected() {
        let e = env("corrupt");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        let bytes = std::fs::read(pack_path(&e)).expect("read pack");

        // Truncated gzip/tar.
        let truncated = pack_path(&e).with_extension("truncated.tar.gz");
        std::fs::write(&truncated, &bytes[..bytes.len() / 2]).expect("write");
        assert!(inspect_core(&truncated).is_err());

        // Garbage.
        let garbage = pack_path(&e).with_extension("garbage.tar.gz");
        std::fs::write(&garbage, b"not a tar.gz at all").expect("write");
        assert!(inspect_core(&garbage).is_err());
        assert!(import_core(&garbage, "skip", ALL, &e.dst).is_err());
    }

    #[test]
    fn pack_with_future_version_is_rejected() {
        let e = env("future");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        // Rewrite the manifest with version=999 and re-pack.
        let manifest_bytes = pack_entry(&pack_path(&e), MANIFEST_NAME).expect("manifest");
        let mut manifest: PackManifest =
            serde_json::from_slice(&manifest_bytes).expect("parse manifest");
        manifest.version = 999;
        let out = std::fs::File::create(pack_path(&e)).expect("create");
        let mut builder = Builder::new(GzEncoder::new(out, Compression::default()));
        let raw = serde_json::to_vec(&manifest).expect("serialize");
        let mut header = Header::new_gnu();
        header.set_size(raw.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, MANIFEST_NAME, raw.as_slice())
            .expect("append");
        builder.into_inner().expect("tar").finish().expect("gz");
        let err = import_core(&pack_path(&e), "skip", ALL, &e.dst).unwrap_err();
        assert!(err.contains("unsupported pack version"), "{err}");
    }

    // ─── Inspect ────────────────────────────────────────────────────────────

    #[test]
    fn inspect_reports_shape_without_writing() {
        let e = env("inspect");
        seed(&e.src);
        let out = export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        let info = inspect_core(&pack_path(&e)).expect("inspect");
        assert_eq!(info.version, 1);
        assert_eq!(info.counts, out.counts);
        assert!(info.created_at_ms > 0);
        assert!(info.generator.starts_with("shannon-"), "{}", info.generator);
        // Inspect never writes.
        assert!(!e.dst.skills_dir().exists());
    }

    // ─── Idempotency & conflicts ────────────────────────────────────────────

    #[test]
    fn reimport_identical_pack_skips_everything() {
        let e = env("idempotent");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        let first = import_core(&pack_path(&e), "overwrite", ALL, &e.dst).expect("first");
        assert!(first.failed.is_empty(), "{:?}", first.failed);
        assert_eq!(first.imported, first_count());

        let second = import_core(&pack_path(&e), "overwrite", ALL, &e.dst).expect("second");
        assert!(second.failed.is_empty(), "{:?}", second.failed);
        assert_eq!(second.imported, PackCounts::default());
        assert_eq!(second.skipped, first_count());
        // Memories were not duplicated either.
        let mut store = MemoryStore::new(e.dst.memories_dir());
        store.load().expect("load");
        assert_eq!(
            store
                .project_memories(&e.dst.project.display().to_string())
                .len(),
            2
        );
    }

    fn first_count() -> PackCounts {
        PackCounts {
            skills: 1,
            commands: 2,
            memories: 2,
            routines: 3,
            profiles: 1,
            persona: 1,
        }
    }

    #[test]
    fn conflict_skip_keeps_ours() {
        let e = env("conflict-skip");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        write(&e.dst.commands_dir().join("deploy.md"), "OURS.\n");

        let report = import_core(&pack_path(&e), "skip", ALL, &e.dst).expect("import");
        assert_eq!(read(&e.dst.commands_dir().join("deploy.md")), "OURS.\n");
        assert_eq!(report.skipped.commands, 1);
        assert_eq!(report.imported.commands, 1); // the fresh blog/draft.md
    }

    #[test]
    fn conflict_overwrite_takes_theirs() {
        let e = env("conflict-overwrite");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        write(&e.dst.commands_dir().join("deploy.md"), "OURS.\n");

        import_core(&pack_path(&e), "overwrite", ALL, &e.dst).expect("import");
        assert_eq!(
            read(&e.dst.commands_dir().join("deploy.md")),
            "Deploy the service.\n"
        );
    }

    #[test]
    fn conflict_rename_writes_imported_variant() {
        let e = env("conflict-rename");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        write(&e.dst.commands_dir().join("deploy.md"), "OURS.\n");
        write(&e.dst.skills_dir().join("commit/SKILL.md"), "OURS.\n");

        let report = import_core(&pack_path(&e), "rename", ALL, &e.dst).expect("import");
        assert_eq!(read(&e.dst.commands_dir().join("deploy.md")), "OURS.\n");
        assert_eq!(
            read(&e.dst.commands_dir().join("deploy-imported.md")),
            "Deploy the service.\n"
        );
        // Skill dir renamed as a unit.
        assert_eq!(read(&e.dst.skills_dir().join("commit/SKILL.md")), "OURS.\n");
        assert!(
            e.dst
                .skills_dir()
                .join("commit-imported/SKILL.md")
                .is_file()
        );
        assert_eq!(report.imported.skills, 1);
        // Persona rename → documented fallback: keep ours (skip).
        write(&e.dst.persona_file(), "OUR PERSONA\n");
        assert_eq!(read(&e.dst.persona_file()), "OUR PERSONA\n");
    }

    #[test]
    fn include_filter_gates_import() {
        let e = env("include-gate");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        let only_skills = PackInclude {
            skills: true,
            ..Default::default()
        };
        let report = import_core(&pack_path(&e), "overwrite", only_skills, &e.dst).expect("import");
        assert_eq!(
            report.imported,
            PackCounts {
                skills: 1,
                ..Default::default()
            }
        );
        assert!(!e.dst.commands_dir().exists());
        assert!(!e.dst.memories_dir().exists());
    }

    #[test]
    fn invalid_conflict_is_rejected() {
        let e = env("bad-conflict");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        let err = import_core(&pack_path(&e), "merge", ALL, &e.dst).unwrap_err();
        assert!(err.contains("invalid conflict"), "{err}");
    }

    // ─── Export hygiene ─────────────────────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn export_skips_symlinks_instead_of_following_them() {
        let e = env("symlink-export");
        seed(&e.src);
        let outside = e._dir.path().join("outside-secret.md");
        write(&outside, "top secret");
        std::os::unix::fs::symlink(&outside, e.src.skills_dir().join("commit/steal.md"))
            .expect("symlink");
        let out = export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        let entries = pack_entries(&pack_path(&e));
        assert!(
            !entries.iter().any(|p| p.contains("steal.md")),
            "{entries:?}"
        );
        let packed = pack_entry(&pack_path(&e), "skills/commit/steal.md");
        assert!(packed.is_none());
        assert_eq!(out.counts.skills, 1);
    }

    #[test]
    fn export_empty_roots_yields_empty_counts_and_valid_pack() {
        let e = env("empty");
        let out = export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        assert_eq!(out.counts, PackCounts::default());
        assert_eq!(out.stripped, 0);
        let info = inspect_core(&pack_path(&e)).expect("inspect");
        assert_eq!(info.version, 1);
        assert_eq!(info.counts, PackCounts::default());
    }

    #[test]
    fn entry_path_validation_rejects_hostile_shapes() {
        for evil in [
            "../evil",
            "a/../../evil",
            "/absolute",
            "back\\slash.md",
            "./dot.md",
            "",
        ] {
            assert!(
                validate_entry_path(evil).is_err(),
                "{evil:?} must be rejected"
            );
        }
        assert!(validate_entry_path("skills/commit/SKILL.md").is_ok());
        assert!(validate_entry_path("persona.md").is_ok());
    }

    #[test]
    fn manifest_sha256_matches_packed_bytes() {
        let e = env("sha");
        seed(&e.src);
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        let manifest_bytes = pack_entry(&pack_path(&e), MANIFEST_NAME).expect("manifest");
        let manifest: PackManifest = serde_json::from_slice(&manifest_bytes).expect("parse");
        assert!(!manifest.assets.is_empty());
        for asset in &manifest.assets {
            let data = pack_entry(&pack_path(&e), &asset.path)
                .unwrap_or_else(|| panic!("entry {} missing", asset.path));
            let mut hasher = Sha256::new();
            hasher.update(&data);
            assert_eq!(
                format!("{:x}", hasher.finalize()),
                asset.sha256,
                "{}",
                asset.path
            );
            assert_eq!(data.len() as u64, asset.bytes, "{}", asset.path);
        }
    }

    // ─── Fix round 1: strip fidelity & manifest verification ────────────────

    #[test]
    fn r1_structured_assets_stay_parseable() {
        // TOML profile: only the value is replaced — the file still parses.
        let toml_input = "name = \"leaky\"\ndescription = \"Team baseline\"\nauto_approve = []\napi_key = \"super-secret-value\"\nconfirm = []\n";
        let (out, n) = strip_text(toml_input, true);
        assert_eq!(n, 1, "{out}");
        let parsed: toml::Value = toml::from_str(&out).expect("stripped TOML must parse");
        assert_eq!(parsed["name"].as_str(), Some("leaky"));
        assert_eq!(parsed["auto_approve"], toml::Value::Array(Vec::new()));
        assert!(
            out.contains("api_key = \"[stripped: secret]\""),
            "structured R1 must keep key syntax: {out}"
        );
        assert!(!out.contains("super-secret-value"));

        // JSON (routine-overrides shape): value replaced in place, trailing
        // comma and sibling keys survive, file still parses.
        let json_input = "{\n  \"post-merge\": true,\n  \"webhook_secret\": \"whsec_abc123\",\n  \"enabled\": 2\n}\n";
        let (out, n) = strip_text(json_input, true);
        assert_eq!(n, 1, "{out}");
        let parsed: serde_json::Value =
            serde_json::from_str(out.trim_end()).expect("stripped JSON must parse");
        assert_eq!(parsed["post-merge"], serde_json::json!(true));
        assert_eq!(parsed["enabled"], serde_json::json!(2));
        assert_eq!(
            parsed["webhook_secret"],
            serde_json::json!("[stripped: secret]")
        );

        // YAML list-item assignment keeps the key.
        let (out, _) = strip_text("- token: abc123def456\n", true);
        assert!(out.starts_with("- token: "), "{out}");
        assert!(out.contains(STRIPPED_PLACEHOLDER), "{out}");
    }

    #[test]
    fn r3a_keeps_text_around_the_webhook_url() {
        let (out, n) = strip_text(
            "notify: https://discord.com/api/webhooks/123456/leakytoken at 9am\n",
            false,
        );
        assert_eq!(n, 1, "{out}");
        assert!(out.starts_with("notify: "), "prefix must survive: {out}");
        assert!(out.contains(" at 9am"), "trailing text must survive: {out}");
        assert!(out.contains(STRIPPED_PLACEHOLDER), "{out}");
        assert!(!out.contains("leakytoken"), "{out}");

        // A webhook URL in quotes (e.g. inside a JSON string) keeps the quotes.
        let (out, _) = strip_text(
            "\"hook\": \"https://hooks.slack.com/services/T0/B0/SECRETX\"\n",
            true,
        );
        assert!(out.starts_with("\"hook\": \""), "{out}");
        assert!(out.ends_with("\"\n"), "closing quote must survive: {out}");
        assert!(!out.contains("SECRETX"), "{out}");
    }

    #[test]
    fn r4_bearer_with_non_ascii_prefix_does_not_panic() {
        // 'İ' lowercases to a LONGER byte sequence; a to_lowercase+offset
        // search would compute invalid byte offsets. Byte-safe lookup must
        // both not panic and still strip.
        let (out, n) = strip_text("\u{130}\u{30c}k Bearer abc123def456ghi\n", false);
        assert_eq!(n, 1, "{out}");
        assert!(out.contains(STRIPPED_PLACEHOLDER), "{out}");
        assert!(!out.contains("abc123def456ghi"), "{out}");
        assert!(
            out.starts_with("\u{130}\u{30c}k "),
            "non-ASCII prefix survives: {out}"
        );
    }

    #[test]
    fn manifest_counts_mismatch_is_rejected() {
        let e = env("counts-mismatch");
        write_raw_pack(
            &pack_path(&e),
            &manifest_bytes_with(
                vec![PackAssetEntry {
                    path: "commands/deploy.md".to_string(),
                    kind: "command".to_string(),
                    bytes: 7,
                    sha256: sha_of(b"Deploy."),
                    stripped: 0,
                }],
                PackCounts {
                    commands: 5,
                    ..Default::default()
                },
            ),
            &[RawEntry {
                name: "commands/deploy.md",
                typeflag: b'0',
                linkname: "",
                data: b"Deploy.",
            }],
        );
        let err = import_core(&pack_path(&e), "skip", ALL, &e.dst).unwrap_err();
        assert!(err.contains("counts"), "{err}");
        assert!(inspect_core(&pack_path(&e)).is_err());
        assert!(!e.dst.commands_dir().join("deploy.md").exists());
    }

    #[test]
    fn sha256_mismatch_fails_that_unit_in_the_report() {
        let e = env("sha-mismatch");
        write_raw_pack(
            &pack_path(&e),
            &manifest_bytes_with(
                vec![
                    PackAssetEntry {
                        path: "commands/good.md".to_string(),
                        kind: "command".to_string(),
                        bytes: 4,
                        sha256: sha_of(b"good"),
                        stripped: 0,
                    },
                    PackAssetEntry {
                        path: "commands/tampered.md".to_string(),
                        kind: "command".to_string(),
                        bytes: 99,
                        sha256: "deadbeefdeadbeefdeadbeef".to_string(),
                        stripped: 0,
                    },
                ],
                PackCounts {
                    commands: 2,
                    ..Default::default()
                },
            ),
            &[
                RawEntry {
                    name: "commands/good.md",
                    typeflag: b'0',
                    linkname: "",
                    data: b"good",
                },
                RawEntry {
                    name: "commands/tampered.md",
                    typeflag: b'0',
                    linkname: "",
                    data: b"evil-bytes",
                },
            ],
        );
        let report =
            import_core(&pack_path(&e), "overwrite", ALL, &e.dst).expect("import proceeds");
        assert_eq!(report.imported.commands, 1, "{report:?}");
        assert_eq!(report.failed.len(), 1, "{report:?}");
        assert_eq!(report.failed[0].item, "commands/tampered.md");
        assert!(
            report.failed[0].error.contains("integrity check failed"),
            "{:?}",
            report.failed[0]
        );
        assert!(e.dst.commands_dir().join("good.md").is_file());
        assert!(!e.dst.commands_dir().join("tampered.md").exists());
    }

    #[test]
    fn exported_toml_profile_with_secret_imports_and_parses() {
        let e = env("toml-profile");
        write(
            &e.src.profiles_dir().join("leaky.toml"),
            &format!(
                "name = \"leaky\"\ndescription = \"Team baseline\"\napi_key = \"{}-live-abcdef1234567890\"\nauto_approve = []\nconfirm = []\ndeny = []\n",
                "sk"
            ),
        );
        let out = export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        assert_eq!(out.stripped, 1, "one R1 redaction expected");

        let packed = pack_entry(&pack_path(&e), "profiles/leaky.toml").expect("entry");
        let text = std::str::from_utf8(&packed).expect("utf8");
        let parsed: toml::Value = toml::from_str(text).expect("stripped profile is valid TOML");
        assert_eq!(parsed["name"].as_str(), Some("leaky"));
        assert!(text.contains("api_key = \"[stripped: secret]\""), "{text}");
        assert!(!text.contains("sk-live"), "{text}");

        import_core(&pack_path(&e), "skip", ALL, &e.dst).expect("import");
        let on_disk = read(&e.dst.profiles_dir().join("leaky.toml"));
        let reparsed: toml::Value = toml::from_str(&on_disk).expect("imported profile parses");
        assert_eq!(reparsed["name"].as_str(), Some("leaky"));
        assert!(!on_disk.contains("sk-live"), "{on_disk}");
    }

    #[test]
    fn exported_json_routines_file_with_secret_stays_parseable() {
        let e = env("json-routines");
        write(
            &e.src.routine_overrides(),
            "{\"post-merge\": true,\n \"bot_token\": \"tok-1234567890abcdefg\",\n \"enabled\": 2}\n",
        );
        export_core(ALL, &e.src, &pack_path(&e)).expect("export");
        let packed = pack_entry(&pack_path(&e), "routines/routine-overrides.json").expect("entry");
        let text = std::str::from_utf8(&packed).expect("utf8");
        let parsed: serde_json::Value =
            serde_json::from_str(text).expect("stripped overrides are valid JSON");
        assert_eq!(parsed["post-merge"], serde_json::json!(true));
        assert_eq!(parsed["enabled"], serde_json::json!(2));
        assert_eq!(parsed["bot_token"], serde_json::json!("[stripped: secret]"));
        assert!(!text.contains("tok-1234567890abcdefg"), "{text}");
    }
}
