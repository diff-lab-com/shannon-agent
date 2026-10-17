//! P0-3: the effective-assembly dump — one JSON view of what is actually
//! mounted at runtime, as opposed to what configuration *claims*.
//!
//! Five views, assembled by [`build_assembly_dump`]:
//!
//! - **tools** — every registered tool with its mounting flags
//!   ([`crate::tools::ToolRegistry::list_tools_runtime`]) plus a *derived*
//!   [`Source`]. Source is name-prefix inference (`mcp__{owner}__{tool}`),
//!   **not** a registration-time declaration: nothing in the `Tool` trait
//!   records provenance, so a custom MCP server that happens to share a
//!   name with an installed plugin is mis-attributed. This is a known,
//!   documented approximation — acceptable for a diagnostic dump.
//! - **hooks** — the mount table enumeratable at runtime from
//!   `shannon_engine::hooks::HookManager` (`hooks_file()` /
//!   `configured_event_types()`): per-event matcher groups and their
//!   commands, plus the config paths they were loaded from.
//! - **skills** — the token-budget trim (`SkillBudgetReport`, from
//!   `shannon_skills`), including what got truncated and what was dropped
//!   entirely.
//! - **compaction** — the three coexisting threshold sources, listed
//!   honestly side by side (see [`CompactionSection`]).
//! - **sandbox** — resolved `SandboxMode`-family mode, world kind and
//!   degrade notices (see [`SandboxSnapshot`]).
//!
//! Degradation contract (P0-3): every source is `Option`; a missing source
//! renders as the JSON string `"unavailable"` and sets
//! [`AssemblyDump::partial`]. This function never panics on missing input.
//!
//! Exposure note: the HTTP face is `GET /api/assembly` on
//! [`crate::api_server::ShannonApiServer`] (embedder-supplied sources via
//! `ShannonApiServer::with_assembly_sources`) and on the `shannon-server`
//! axum stack behind `shannon serve` (serve-time sources only: a bare tool
//! registry — served sessions register no tools — best-effort hooks and
//! engine-default compaction; skills/sandbox degrade). Sources a host
//! never wired degrade to `"unavailable"`/`partial` rather than erroring.
//!
//! World-kind note: the sandbox world handles built by the §4.12 assemblies
//! are consumed by tool registration and not retained
//! (`crates/shannon-tools/src/lib.rs` `register_all_tools`), so callers
//! re-`detect()` and re-`assemble()` to recover the backend kind — see
//! [`SandboxSnapshot`].

use crate::query_engine::QueryEngineConfig;
use crate::tools::ToolRegistry;
use serde::{Serialize, Serializer};
use shannon_engine::compact::CompactConfig;
use shannon_engine::hooks::{HookManager, HookType};
use shannon_skills::{SkillRegistry, SkillSource};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Hardcoded pre-compaction warning floor in the agent-loop ladder: fires a
/// Progress event plus a synthetic user message when usage crosses 60%
/// (capped at `compression_threshold`). Byte-for-byte a literal in
/// `crates/shannon-core/src/query_engine/engine/agent_loop.rs` — it has no
/// named constant to import, so the dump re-states it here and the compaction
/// section carries this file's provenance comment.
pub const AGENT_LOOP_PRE_COMPACT_WARNING_RATIO: f32 = 0.6;

/// Hardcoded "compaction imminent" warning in the same agent-loop ladder
/// (fires once above 80% usage). Same provenance story as
/// [`AGENT_LOOP_PRE_COMPACT_WARNING_RATIO`].
pub const AGENT_LOOP_IMMINENT_WARNING_RATIO: f32 = 0.8;

// ---------------------------------------------------------------------------
// Source derivation (name-prefix heuristic — documented approximation)
// ---------------------------------------------------------------------------

/// Where a mounted tool came from, derived from its registered name.
///
/// **This is name inference, not registration-time provenance.** The
/// derivation rule (P0-3):
///
/// - `mcp__tool_search` (exact) → [`Source::ToolSearch`] — the deferred-tool
///   discovery helper auto-registered by `ToolRegistry::register_batch`;
/// - `mcp__{owner}__{tool}` with `owner` in the known plugin set →
///   [`Source::Plugin`] (the same shape `crate::plugin::permissions::
///   owner_of_tool` parses);
/// - any other `mcp__{owner}__{tool}` / `mcp__{owner}` → [`Source::Mcp`]
///   with the owner as server name;
/// - no `mcp__` prefix → [`Source::Builtin`].
///
/// A custom `.mcp.json` server sharing a name with an installed plugin
/// mis-reports as `Plugin` — accepted for dump purposes (plan §Task 3
/// risk note).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// Registered without an `mcp__` prefix (standard tool or skill adapter).
    Builtin,
    /// MCP tool from a plain (non-plugin) server `mcp__{server}__*`.
    Mcp {
        /// MCP server name parsed from the tool name.
        server: String,
    },
    /// Tool from an installed plugin namespace `mcp__{plugin}__*`.
    Plugin {
        /// Plugin name parsed from the tool name.
        plugin: String,
    },
    /// The deferred-tool discovery helper (`mcp__tool_search`).
    ToolSearch,
}

/// Derive [`Source`] from a registered tool name against the known plugin
/// set. See the type docs for the rule and its limits.
pub fn derive_source(tool_name: &str, known_plugins: &[&str]) -> Source {
    if tool_name == "mcp__tool_search" {
        return Source::ToolSearch;
    }
    let Some(rest) = tool_name.strip_prefix("mcp__") else {
        return Source::Builtin;
    };
    if rest.is_empty() {
        // Degenerate "mcp__" — nothing to attribute; call it builtin.
        return Source::Builtin;
    }
    let Some(sep) = rest.find("__") else {
        // Owner-shaped name with no tool segment ("mcp__foo"): the
        // remainder is the best-effort server attribution.
        return Source::Mcp {
            server: rest.to_string(),
        };
    };
    let owner = &rest[..sep];
    if owner.is_empty() {
        // Degenerate "mcp____x" — no attributable owner.
        return Source::Builtin;
    }
    if known_plugins.contains(&owner) {
        Source::Plugin {
            plugin: owner.to_string(),
        }
    } else {
        Source::Mcp {
            server: owner.to_string(),
        }
    }
}

/// Cheap runtime derivation of the installed-plugin name set: subdirectories
/// of `plugins_dir` carrying a manifest file (`plugin.toml`, or
/// `.claude-plugin/plugin.json`) — the same two shapes
/// `crate::plugin::PluginRegistry::load_all` probes. Names only; manifests
/// are not parsed here (a dump must not pay full plugin loading).
///
/// The default directory matches the CLI convention:
/// `~/.shannon/plugins`.
pub fn known_plugin_names(plugins_dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(plugins_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let has_manifest =
            path.join("plugin.toml").is_file() || path.join(".claude-plugin/plugin.json").is_file();
        if has_manifest {
            out.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

/// One assembly view: fully rendered, or the JSON string `"unavailable"`
/// when the caller could not supply that source (P0-3 degradation
/// contract). Every `Unavailable` section sets [`AssemblyDump::partial`].
#[derive(Debug, Clone)]
pub enum Section<T> {
    /// The source was supplied and has been projected.
    Available(T),
    /// The source was not supplied by the caller.
    Unavailable,
}

impl<T: Serialize> Serialize for Section<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Section::Available(v) => v.serialize(serializer),
            Section::Unavailable => serializer.serialize_str("unavailable"),
        }
    }
}

impl<T> Section<T> {
    /// True when this view degraded to `unavailable`.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Section::Unavailable)
    }
}

/// Tools view: the full mounted set with derived sources.
#[derive(Debug, Clone, Serialize)]
pub struct ToolsSection {
    /// Number of mounted tools (== `tools.len()`).
    pub count: usize,
    /// How many are deferred (schema-hidden, discoverable via search).
    pub deferred_count: usize,
    /// One entry per mounted tool, sorted by name for determinism.
    pub tools: Vec<ToolAssemblyInfo>,
}

/// One tool row: runtime flags plus the name-derived [`Source`].
#[derive(Debug, Clone, Serialize)]
pub struct ToolAssemblyInfo {
    /// Registered tool name.
    pub name: String,
    /// Tool category.
    pub category: String,
    /// No side effects (cacheable / parallelizable).
    pub read_only: bool,
    /// May perform irreversible changes.
    pub destructive: bool,
    /// Schema excluded from the LLM tool list.
    pub hidden_from_llm: bool,
    /// Schema-hidden until discovered via `mcp__tool_search`.
    pub deferred: bool,
    /// Name-derived provenance — see [`Source`] for the approximation.
    pub source: Source,
}

/// Hooks view: the runtime-enumerable mount table.
#[derive(Debug, Clone, Serialize)]
pub struct HooksSection {
    /// User-level hooks config the manager loaded from.
    pub user_config_path: String,
    /// Project-level hooks config the manager loaded from.
    pub project_config_path: String,
    /// Configured event kinds (raw config keys, sorted — unknown kinds
    /// included on purpose: they are mounted even if unroutable).
    pub configured_events: Vec<String>,
    /// Total number of mounted hook commands.
    pub mount_count: usize,
    /// Per-matcher mounts, sorted by (event, matcher, command).
    pub mounts: Vec<HookMount>,
}

/// One hook mount: an event kind + matcher group with its command(s).
#[derive(Debug, Clone, Serialize)]
pub struct HookMount {
    /// Event kind as written in config (raw key).
    pub event: String,
    /// Matcher pattern (exact / `"*"` / pipe-separated / regex).
    pub matcher: String,
    /// Conditional gate (`"Bash(rm *)"`-style), when configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    /// Execution strategy (`command` / `http` / `llm` / `prompt` / ...).
    pub hook_type: String,
    /// Shell command (or prompt template / URL payload, per type).
    pub command: String,
    /// Per-hook timeout in seconds.
    pub timeout_secs: u64,
    /// Whether the hook blocks the pipeline on its result.
    pub blocking: bool,
}

fn hook_type_str(t: &HookType) -> &'static str {
    match t {
        HookType::Command => "command",
        HookType::Http => "http",
        HookType::Llm => "llm",
        HookType::Prompt => "prompt",
        HookType::McpTool => "mcp_tool",
        HookType::Agent => "agent",
    }
}

fn skill_source_str(s: &SkillSource) -> &'static str {
    match s {
        SkillSource::Bundled => "bundled",
        SkillSource::User => "user",
        SkillSource::Project => "project",
        SkillSource::Managed => "managed",
        SkillSource::Mcp => "mcp",
        SkillSource::CommandsDeprecated => "commands_deprecated",
        SkillSource::Plugin => "plugin",
    }
}

/// Skills view: the token-budget trim with its casualties.
#[derive(Debug, Clone, Serialize)]
pub struct SkillsSection {
    /// Token budget the trim ran against.
    pub budget: usize,
    /// Where the budget number comes from.
    pub budget_source: &'static str,
    /// Candidates before trimming (non-hidden registered skills).
    pub registered: usize,
    /// Number of skills actually mounted (`included.len()`).
    pub included_count: usize,
    /// Mounted skills, alphabetical (mirrors the budget pass order).
    pub included: Vec<SkillEntry>,
    /// Names whose description was truncated to fit (marked with `...`).
    pub truncated: Vec<String>,
    /// Names dropped entirely by the budget.
    pub dropped: Vec<String>,
}

/// One mounted skill, projected for the dump.
#[derive(Debug, Clone, Serialize)]
pub struct SkillEntry {
    /// Display name (invocation key).
    pub name: String,
    /// Short description (possibly truncated by the budget).
    pub description: String,
    /// Estimated context tokens (`SkillMetadata::estimated_tokens`).
    pub estimated_tokens: usize,
    /// Load origin (`user` / `project` / `bundled` / ...).
    pub source: String,
}

/// Compaction view: every threshold that coexists in the harness, with
/// provenance. Three *places* decide "when does context compact", and they
/// do not agree by design (yet) — this dump lists them side by side rather
/// than picking a winner:
///
/// 1. the agent-loop warning ladder — **hardcoded literals** 60% / 80% in
///    `query_engine/engine/agent_loop.rs` (re-stated here as
///    [`AGENT_LOOP_PRE_COMPACT_WARNING_RATIO`] /
///    [`AGENT_LOOP_IMMINENT_WARNING_RATIO`]; the source has no named
///    constants to import);
/// 2. [`QueryEngineConfig::compression_threshold`] — the actual
///    auto-compaction trigger the loop enforces (default 0.8);
/// 3. `CompactConfig::default().trigger_threshold` (default 0.75) — the
///    standalone `CompactEngine`'s own auto-compact trigger, an independent
///    path from the loop ladder.
#[derive(Debug, Clone, Serialize)]
pub struct CompactionSection {
    /// Master switch gating the loop's compaction ladder (config).
    pub auto_compact_enabled: bool,
    /// All threshold sources, in the order listed above.
    pub thresholds: Vec<CompactionThreshold>,
}

/// One threshold row: value + provenance + what it actually controls.
#[derive(Debug, Clone, Serialize)]
pub struct CompactionThreshold {
    /// Stable identifier (`agent_loop_warning_floor`, ...).
    pub name: &'static str,
    /// Usage ratio (0-1), quantized to 3 decimals — f32 ratios render as
    /// clean decimals instead of `0.6000000238418579`.
    pub value: f64,
    /// Where the number lives in the codebase.
    pub provenance: &'static str,
    /// What behavior it drives.
    pub role: &'static str,
}

/// Quantize an f32 ratio to 3 decimals (see [`CompactionThreshold::value`]).
/// The division happens in f64 — casting first avoids re-importing the f32
/// representation error (`0.6f32 as f64` is `0.6000000238418579`).
fn quantize(x: f32) -> f64 {
    let thousandths = (x * 1000.0).round() as f64;
    thousandths / 1000.0
}

/// Sandbox view: the resolved settings plus the assembled world's identity.
///
/// `SandboxSettings` itself lives in `shannon-tools`, which depends on this
/// crate — so the dump consumes this neutral snapshot and the host (CLI /
/// API embedder) builds it from `shannon_tools::sandbox::SandboxSettings`
/// and its assembly result. Carries the world `kind` ("local" / "landlock")
/// and the assembly's degrade notices: world handles are not retained by
/// tool registration, so this snapshot is the only honest record of what
/// was assembled.
#[derive(Debug, Clone, Serialize)]
pub struct SandboxSnapshot {
    /// Configured mode token (`off` / `local` / `landlock`).
    pub mode: String,
    /// Whether children may use TCP networking.
    pub network: bool,
    /// Additional writable roots.
    pub extra_writable: Vec<String>,
    /// Additional readable roots.
    pub extra_readable: Vec<String>,
    /// Additional executable roots.
    pub extra_executable: Vec<String>,
    /// Assembled backend identity (`"local"` / `"landlock"`); `None` when
    /// mode is `off` (legacy passthrough — no world was assembled) or the
    /// requested world failed to assemble.
    pub world_kind: Option<String>,
    /// Non-fatal degrades from world assembly (`tag: detail`).
    pub notices: Vec<String>,
}

/// The full assembly dump (P0-3).
#[derive(Debug, Clone, Serialize)]
pub struct AssemblyDump {
    /// RFC 3339 timestamp of the dump (redact in golden snapshots).
    pub generated_at: String,
    /// Mounted tools with derived sources.
    pub tools: Section<ToolsSection>,
    /// Hook mount table.
    pub hooks: Section<HooksSection>,
    /// Skill budget trim (with dropped/truncated).
    pub skills: Section<SkillsSection>,
    /// Coexisting compaction thresholds with provenance.
    pub compaction: Section<CompactionSection>,
    /// Sandbox mode / world kind / degrade notices.
    pub sandbox: Section<SandboxSnapshot>,
    /// True when at least one source was unavailable.
    pub partial: bool,
    /// Free-form notes (degradation reasons, dump-mode caveats).
    pub notes: Vec<String>,
}

/// Non-registry sources an embedder (CLI dump handler, API host) can hand to
/// `ShannonApiServer::with_assembly_sources` so `GET /api/assembly` renders
/// beyond the tool registry. Every field optional — absent fields degrade.
#[derive(Debug, Clone, Default)]
pub struct AssemblySources {
    /// Loaded hook manager for the hooks mount table.
    pub hooks: Option<Arc<HookManager>>,
    /// Skill registry for the budget-trim section.
    pub skills: Option<Arc<SkillRegistry>>,
    /// Effective engine config for the compaction projection.
    pub config: Option<QueryEngineConfig>,
    /// Resolved sandbox settings + world kind + notices.
    pub sandbox: Option<SandboxSnapshot>,
    /// Installed plugin names for source attribution (see
    /// [`known_plugin_names`]); empty = every `mcp__*` owner reports as a
    /// plain MCP server.
    pub known_plugins: Vec<String>,
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Build the assembly dump from whatever sources the caller can supply.
///
/// All sources are optional; a missing one renders `"unavailable"` in its
/// section, sets [`AssemblyDump::partial`], and appends a note. The skills
/// budget uses [`SkillRegistry::DEFAULT_SKILL_TOKEN_BUDGET`] — the same
/// budget the LLM context injection uses
/// (`SkillRegistry::format_skills_for_llm`).
pub fn build_assembly_dump(
    registry: Option<&ToolRegistry>,
    hooks: Option<&HookManager>,
    skills: Option<&SkillRegistry>,
    config: Option<&QueryEngineConfig>,
    sandbox: Option<&SandboxSnapshot>,
    known_plugins: &[&str],
) -> AssemblyDump {
    let mut partial = false;
    let mut notes = Vec::new();

    let tools = match registry {
        Some(r) => Section::Available(tools_section(r, known_plugins)),
        None => {
            partial = true;
            notes.push("tools: ToolRegistry not supplied".to_string());
            Section::Unavailable
        }
    };

    let hooks_section = match hooks {
        Some(h) => Section::Available(hooks_section(h)),
        None => {
            partial = true;
            notes.push("hooks: HookManager not supplied".to_string());
            Section::Unavailable
        }
    };

    let skills_section = match skills {
        Some(s) => Section::Available(skills_section(s)),
        None => {
            partial = true;
            notes.push("skills: SkillRegistry not supplied".to_string());
            Section::Unavailable
        }
    };

    let compaction = match config {
        Some(c) => Section::Available(compaction_section(c)),
        None => {
            partial = true;
            notes.push("compaction: QueryEngineConfig not supplied".to_string());
            Section::Unavailable
        }
    };

    let sandbox_section = match sandbox {
        Some(s) => Section::Available(s.clone()),
        None => {
            partial = true;
            notes.push("sandbox: settings not supplied".to_string());
            Section::Unavailable
        }
    };

    AssemblyDump {
        generated_at: chrono::Utc::now().to_rfc3339(),
        tools,
        hooks: hooks_section,
        skills: skills_section,
        compaction,
        sandbox: sandbox_section,
        partial,
        notes,
    }
}

/// Project the registry + derived sources into the tools section.
fn tools_section(registry: &ToolRegistry, known_plugins: &[&str]) -> ToolsSection {
    let mut tools: Vec<ToolAssemblyInfo> = registry
        .list_tools_runtime()
        .into_iter()
        .map(|t| {
            let source = derive_source(&t.name, known_plugins);
            ToolAssemblyInfo {
                name: t.name,
                category: t.category,
                read_only: t.read_only,
                destructive: t.destructive,
                hidden_from_llm: t.hidden_from_llm,
                deferred: t.deferred,
                source,
            }
        })
        .collect();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    let deferred_count = tools.iter().filter(|t| t.deferred).count();
    ToolsSection {
        count: tools.len(),
        deferred_count,
        tools,
    }
}

/// Project the hook manager's loaded config into the hooks section.
fn hooks_section(manager: &HookManager) -> HooksSection {
    let file = manager.hooks_file();
    // BTreeMap for deterministic (event → group) ordering.
    let events: BTreeMap<&String, &Vec<shannon_engine::hooks::HookConfig>> =
        file.hooks.iter().collect();

    let mut mounts = Vec::new();
    for (event, groups) in &events {
        for group in groups.iter() {
            for def in &group.hooks {
                mounts.push(HookMount {
                    event: (*event).clone(),
                    matcher: group.matcher.clone(),
                    if_condition: group.if_condition.clone(),
                    hook_type: hook_type_str(&def.r#type).to_string(),
                    command: def.command.clone(),
                    timeout_secs: def.timeout,
                    blocking: def.blocking,
                });
            }
        }
    }
    mounts.sort_by(|a, b| {
        (&a.event, &a.matcher, &a.command).cmp(&(&b.event, &b.matcher, &b.command))
    });
    let mount_count = mounts.len();
    HooksSection {
        user_config_path: manager.user_config_path().display().to_string(),
        project_config_path: manager.project_config_path().display().to_string(),
        configured_events: events.keys().map(|k| (*k).clone()).collect(),
        mount_count,
        mounts,
    }
}

/// Run the default budget trim and project it into the skills section.
fn skills_section(registry: &SkillRegistry) -> SkillsSection {
    let budget = SkillRegistry::DEFAULT_SKILL_TOKEN_BUDGET;
    let report = registry.available_skills_metadata_with_budget_report(budget);
    // Invariant (shannon-skills budget pass): `truncated ⊆ included` — a
    // truncated skill IS mounted, just with a shortened description — and
    // `dropped ∩ included = ∅`, so candidates = included + dropped.
    // Counting `truncated` too would double-count every trimmed skill.
    let registered = report.included.len() + report.dropped.len();
    let included = report
        .included
        .iter()
        .map(|m| SkillEntry {
            name: m.name.clone(),
            description: m.description.clone(),
            estimated_tokens: m.estimated_tokens(),
            source: skill_source_str(&m.source).to_string(),
        })
        .collect();
    SkillsSection {
        budget,
        budget_source: "SkillRegistry::DEFAULT_SKILL_TOKEN_BUDGET",
        registered,
        included_count: report.included.len(),
        included,
        truncated: report.truncated,
        dropped: report.dropped,
    }
}

/// Project the effective config plus the two out-of-config threshold
/// sources into the compaction section (see [`CompactionSection`]).
fn compaction_section(config: &QueryEngineConfig) -> CompactionSection {
    let compact_default = CompactConfig::default();
    CompactionSection {
        auto_compact_enabled: config.auto_compact_enabled,
        thresholds: vec![
            CompactionThreshold {
                name: "agent_loop_warning_floor",
                value: quantize(AGENT_LOOP_PRE_COMPACT_WARNING_RATIO),
                provenance: "hardcoded literal in query_engine/engine/agent_loop.rs",
                role: "pre-compaction warning: Progress event + synthetic user message once \
                       usage crosses 60% (only below compression_threshold)",
            },
            CompactionThreshold {
                name: "agent_loop_imminent_warning",
                value: quantize(AGENT_LOOP_IMMINENT_WARNING_RATIO),
                provenance: "hardcoded literal in query_engine/engine/agent_loop.rs",
                role: "imminent-compaction warning (synthetic user message, fires once)",
            },
            CompactionThreshold {
                name: "effective_compression_threshold",
                value: quantize(config.compression_threshold),
                provenance: "QueryEngineConfig.compression_threshold \
                             (query_engine/types.rs; default 0.8)",
                role: "the actual auto-compaction trigger enforced by the agent loop",
            },
            CompactionThreshold {
                name: "compact_engine_trigger_threshold",
                value: quantize(compact_default.trigger_threshold),
                provenance: "shannon-engine compact/types.rs CompactConfig::default \
                             (default 0.75)",
                role: "standalone CompactEngine auto-compact trigger — coexists with the \
                       loop ladder, independent path",
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_skills::Skill;
    use shannon_tool_interface::Tool;

    /// Minimal tool fixture with configurable flags.
    struct FixtureTool {
        name: &'static str,
        category: &'static str,
        read_only: bool,
        destructive: bool,
        hidden: bool,
    }

    impl Default for FixtureTool {
        fn default() -> Self {
            Self {
                name: "Read",
                category: "fs",
                read_only: true,
                destructive: false,
                hidden: false,
            }
        }
    }

    #[async_trait::async_trait]
    impl Tool for FixtureTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            "fixture"
        }
        fn category(&self) -> &str {
            self.category
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        fn is_read_only(&self) -> bool {
            self.read_only
        }
        fn is_destructive(&self) -> bool {
            self.destructive
        }
        fn hidden_from_llm(&self) -> bool {
            self.hidden
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
        ) -> Result<shannon_tool_interface::ToolOutput, shannon_tool_interface::ToolError> {
            Ok(shannon_tool_interface::ToolOutput::success("ok".into()))
        }
    }

    // ── Source derivation ───────────────────────────────────────────────

    #[test]
    fn derive_source_builtin_for_unprefixed() {
        assert_eq!(derive_source("Read", &[]), Source::Builtin);
        assert_eq!(derive_source("skill_commit", &[]), Source::Builtin);
    }

    #[test]
    fn derive_source_tool_search_exact_match() {
        assert_eq!(derive_source("mcp__tool_search", &[]), Source::ToolSearch);
    }

    #[test]
    fn derive_source_mcp_server() {
        assert_eq!(
            derive_source("mcp__playwright__click", &[]),
            Source::Mcp {
                server: "playwright".to_string()
            }
        );
    }

    #[test]
    fn derive_source_plugin_wins_for_known_owner() {
        assert_eq!(
            derive_source("mcp__acme__widget", &["acme", "beta"]),
            Source::Plugin {
                plugin: "acme".to_string()
            }
        );
        // Unknown owner of the same shape stays a plain MCP server — the
        // documented approximation edge (name collision mis-attribution).
        assert_eq!(
            derive_source("mcp__acme__widget", &["beta"]),
            Source::Mcp {
                server: "acme".to_string()
            }
        );
    }

    #[test]
    fn derive_source_degenerate_shapes_never_panic() {
        assert_eq!(derive_source("mcp__", &[]), Source::Builtin);
        assert_eq!(derive_source("mcp____x", &[]), Source::Builtin);
        assert_eq!(
            derive_source("mcp__lonely", &[]),
            Source::Mcp {
                server: "lonely".to_string()
            }
        );
    }

    #[test]
    fn known_plugin_names_scans_manifest_shapes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("acme/.claude-plugin")).unwrap();
        std::fs::write(dir.join("acme/.claude-plugin/plugin.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("beta")).unwrap();
        std::fs::write(dir.join("beta/plugin.toml"), "name = \"beta\"").unwrap();
        // No manifest → not a plugin; a plain file → ignored.
        std::fs::create_dir_all(dir.join("scratch")).unwrap();
        std::fs::write(dir.join("loose.txt"), "").unwrap();

        assert_eq!(
            known_plugin_names(dir),
            vec!["acme".to_string(), "beta".to_string()]
        );
        // Missing directory degrades to empty, never panics.
        assert!(known_plugin_names(dir.join("nope").as_path()).is_empty());
    }

    // ── Degradation paths ───────────────────────────────────────────────

    #[test]
    fn all_sources_missing_degrades_without_panicking() {
        let dump = build_assembly_dump(None, None, None, None, None, &[]);
        assert!(dump.partial);
        assert!(dump.tools.is_unavailable());
        assert!(dump.hooks.is_unavailable());
        assert!(dump.skills.is_unavailable());
        assert!(dump.compaction.is_unavailable());
        assert!(dump.sandbox.is_unavailable());
        assert_eq!(dump.notes.len(), 5);
        // The JSON rendering really carries the "unavailable" strings.
        let json = serde_json::to_value(&dump).unwrap();
        assert_eq!(json["tools"], "unavailable");
        assert_eq!(json["hooks"], "unavailable");
        assert_eq!(json["skills"], "unavailable");
        assert_eq!(json["compaction"], "unavailable");
        assert_eq!(json["sandbox"], "unavailable");
    }

    #[test]
    fn single_missing_source_sets_partial() {
        let registry = ToolRegistry::new();
        let dump = build_assembly_dump(Some(&registry), None, None, None, None, &[]);
        assert!(dump.partial);
        assert!(!dump.tools.is_unavailable());
        assert!(dump.hooks.is_unavailable());
        // An empty registry still renders a real (empty) tools section.
        let json = serde_json::to_value(&dump).unwrap();
        assert_eq!(json["tools"]["count"], 0);
        assert_eq!(json["notes"].as_array().map(Vec::len), Some(4));
    }

    // ── Tools section: flags + derived sources ──────────────────────────

    #[test]
    fn tools_section_reports_flags_and_sources() {
        let registry = ToolRegistry::new();
        registry
            .register(Box::new(FixtureTool {
                name: "Read",
                ..FixtureTool::default()
            }))
            .unwrap();
        registry
            .register(Box::new(FixtureTool {
                name: "Bash",
                category: "exec",
                read_only: false,
                destructive: true,
                hidden: false,
            }))
            .unwrap();
        registry
            .register_deferred(Box::new(FixtureTool {
                name: "mcp__github__search_code",
                category: "mcp",
                read_only: true,
                destructive: false,
                hidden: false,
            }))
            .unwrap();
        registry
            .register(Box::new(FixtureTool {
                name: "mcp__acme__widget",
                category: "mcp",
                read_only: true,
                destructive: false,
                hidden: true,
            }))
            .unwrap();

        let section = tools_section(&registry, &["acme"]);
        assert_eq!(section.count, 4);
        assert_eq!(section.deferred_count, 1);

        let by_name = |n: &str| {
            section
                .tools
                .iter()
                .find(|t| t.name == n)
                .unwrap_or_else(|| panic!("missing tool {n}"))
        };

        let read = by_name("Read");
        assert!(read.read_only);
        assert!(!read.destructive);
        assert!(!read.hidden_from_llm);
        assert!(!read.deferred);
        assert_eq!(read.source, Source::Builtin);

        let bash = by_name("Bash");
        assert!(bash.destructive);
        assert_eq!(bash.source, Source::Builtin);

        let search = by_name("mcp__github__search_code");
        assert!(search.deferred, "register_deferred must mark deferred");
        assert_eq!(
            search.source,
            Source::Mcp {
                server: "github".to_string()
            }
        );

        let widget = by_name("mcp__acme__widget");
        assert!(widget.hidden_from_llm);
        assert!(!widget.deferred, "hidden ≠ deferred");
        assert_eq!(
            widget.source,
            Source::Plugin {
                plugin: "acme".to_string()
            }
        );

        // Deterministic ordering (registry stores in a HashMap).
        let names: Vec<&str> = section.tools.iter().map(|t| t.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    // ── Hooks section ───────────────────────────────────────────────────

    /// HookManager loaded from a temp project config (user path pointed at a
    /// nonexistent file — loads as empty by contract).
    fn fixture_hook_manager() -> HookManager {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("user-hooks.json");
        let project = tmp.path().join("project-hooks.json");
        std::fs::write(
            &project,
            r#"{
                "hooks": {
                    "PreToolUse": [
                        {
                            "matcher": "Bash",
                            "hooks": [
                                {"type": "command", "command": "echo audit", "timeout": 15, "blocking": false}
                            ]
                        },
                        {
                            "matcher": "Edit|Write",
                            "ifCondition": "Bash(rm *)",
                            "hooks": [
                                {"type": "command", "command": "echo guard"}
                            ]
                        }
                    ],
                    "SessionStart": [
                        {"matcher": "*", "hooks": [{"type": "prompt", "command": "summarize"}]}
                    ]
                }
            }"#,
        )
        .unwrap();
        let mut mgr = HookManager::with_paths(user, project);
        mgr.load().expect("hooks config loads");
        mgr
    }

    #[test]
    fn hooks_section_renders_mount_table() {
        let mgr = fixture_hook_manager();
        let section = hooks_section(&mgr);
        assert_eq!(section.mount_count, 3);
        assert_eq!(
            section.configured_events,
            vec!["PreToolUse".to_string(), "SessionStart".to_string()]
        );

        let audit = section
            .mounts
            .iter()
            .find(|m| m.command == "echo audit")
            .expect("audit mount");
        assert_eq!(audit.event, "PreToolUse");
        assert_eq!(audit.matcher, "Bash");
        assert_eq!(audit.hook_type, "command");
        assert_eq!(audit.timeout_secs, 15);
        assert!(!audit.blocking);

        let guard = section
            .mounts
            .iter()
            .find(|m| m.command == "echo guard")
            .expect("guard mount");
        assert_eq!(guard.matcher, "Edit|Write");
        assert_eq!(guard.if_condition.as_deref(), Some("Bash(rm *)"));
        assert_eq!(guard.timeout_secs, 30, "default timeout surfaces");
        assert!(guard.blocking, "default blocking surfaces");

        let prompt = section
            .mounts
            .iter()
            .find(|m| m.command == "summarize")
            .expect("prompt mount");
        assert_eq!(prompt.hook_type, "prompt");
        assert_eq!(prompt.matcher, "*");

        // Deterministic ordering.
        let keys: Vec<(&str, &str, &str)> = section
            .mounts
            .iter()
            .map(|m| (m.event.as_str(), m.matcher.as_str(), m.command.as_str()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
    }

    // ── Skills section ──────────────────────────────────────────────────

    #[test]
    fn skills_section_reports_budget_casualties() {
        let registry = SkillRegistry::new();
        for i in 0..6 {
            let skill = Skill::new(
                format!("skill-{i:02}"),
                format!("skill-{i:02}"),
                format!("Description for skill {i} that takes a handful of tokens"),
                "Content".to_string(),
            );
            registry.register(skill).unwrap();
        }
        let section = skills_section(&registry);
        assert_eq!(section.budget, SkillRegistry::DEFAULT_SKILL_TOKEN_BUDGET);
        assert_eq!(section.registered, 6);
        assert_eq!(section.included_count, section.included.len());
        // Default budget (2000 tokens) fits six short skills — nothing cut.
        assert!(section.dropped.is_empty());
        assert!(section.truncated.is_empty());
        // Accounting invariant: registered == included + dropped (a
        // truncated skill counts once, in included).
        assert_eq!(
            section.registered,
            section.included_count + section.dropped.len()
        );
    }

    /// Truncating budget: `truncated` names must be a subset of `included`
    /// (the skill IS mounted, description shortened) and must NOT inflate
    /// `registered` — pre-fix, registered double-counted the truncated row.
    #[test]
    fn skills_section_truncated_skills_count_once() {
        let registry = SkillRegistry::new();
        // Two ~1000-token descriptions: the second overflows the default
        // 2000-token budget with room to truncate (remaining > 0,
        // max_chars > 10) — exactly the double-counting case.
        let long_desc = "w".repeat(4000);
        for name in ["fat_a", "fat_b"] {
            let skill = Skill::new(
                name.to_string(),
                name.to_string(),
                long_desc.clone(),
                "Content".to_string(),
            );
            registry.register(skill).unwrap();
        }

        let section = skills_section(&registry);
        assert_eq!(section.truncated, vec!["fat_b".to_string()]);
        assert_eq!(section.included_count, 2, "truncated skill stays mounted");
        // truncated ⊆ included
        for name in &section.truncated {
            assert!(
                section.included.iter().any(|e| &e.name == name),
                "truncated skill must also be included"
            );
            assert!(
                section
                    .included
                    .iter()
                    .any(|e| &e.name == name && e.description.ends_with("...")),
                "truncated description carries the ... marker"
            );
        }
        assert!(section.dropped.is_empty());
        // registered = included + dropped (NOT + truncated) == 2, not 3.
        assert_eq!(section.registered, 2);
        assert_eq!(
            section.registered,
            section.included_count + section.dropped.len()
        );
    }

    // ── Compaction section ──────────────────────────────────────────────

    #[test]
    fn compaction_section_lists_all_threshold_sources() {
        let config = QueryEngineConfig::default();
        let section = compaction_section(&config);
        assert!(section.auto_compact_enabled);
        let names: Vec<&str> = section.thresholds.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            vec![
                "agent_loop_warning_floor",
                "agent_loop_imminent_warning",
                "effective_compression_threshold",
                "compact_engine_trigger_threshold",
            ]
        );
        let value = |n: &str| {
            section
                .thresholds
                .iter()
                .find(|t| t.name == n)
                .map(|t| t.value)
                .unwrap()
        };
        assert_eq!(value("agent_loop_warning_floor"), 0.6);
        assert_eq!(value("agent_loop_imminent_warning"), 0.8);
        assert_eq!(value("effective_compression_threshold"), 0.8);
        assert_eq!(value("compact_engine_trigger_threshold"), 0.75);
        // Every row carries non-empty provenance.
        assert!(section.thresholds.iter().all(|t| !t.provenance.is_empty()));
    }

    #[test]
    fn quantize_renders_clean_decimals() {
        assert_eq!(quantize(0.6f32), 0.6);
        assert_eq!(quantize(0.8f32), 0.8);
        assert_eq!(quantize(0.75f32), 0.75);
    }

    // ── Golden snapshot (config_dump.rs pattern) ────────────────────────

    /// Deterministic fixture covering every section: tools with distinct
    /// flags/sources, a loaded hook config, skills under a tight-but-real
    /// budget, default engine config, and a landlock-shaped sandbox.
    fn fixture_dump() -> AssemblyDump {
        let registry = ToolRegistry::new();
        registry
            .register(Box::new(FixtureTool {
                name: "Read",
                ..FixtureTool::default()
            }))
            .unwrap();
        registry
            .register(Box::new(FixtureTool {
                name: "Bash",
                category: "exec",
                read_only: false,
                destructive: true,
                hidden: false,
            }))
            .unwrap();
        registry
            .register(Box::new(FixtureTool {
                name: "mcp__acme__widget",
                category: "mcp",
                read_only: true,
                destructive: false,
                hidden: true,
            }))
            .unwrap();
        registry
            .register_deferred(Box::new(FixtureTool {
                name: "mcp__github__search_code",
                category: "mcp",
                read_only: true,
                destructive: false,
                hidden: false,
            }))
            .unwrap();

        let hooks = fixture_hook_manager();

        let skills = SkillRegistry::new();
        let skill = Skill::new(
            "commit".to_string(),
            "commit".to_string(),
            "Create git commits with conventional messages".to_string(),
            "Content".to_string(),
        );
        skills.register(skill).unwrap();

        let config = QueryEngineConfig::default();
        let sandbox = SandboxSnapshot {
            mode: "landlock".to_string(),
            network: false,
            extra_writable: vec!["/tmp/workspace".to_string()],
            extra_readable: vec![],
            extra_executable: vec!["/usr".to_string()],
            world_kind: Some("landlock".to_string()),
            notices: vec!["user-space-only: fs policy enforced in-process".to_string()],
        };

        build_assembly_dump(
            Some(&registry),
            Some(&hooks),
            Some(&skills),
            Some(&config),
            Some(&sandbox),
            &["acme"],
        )
    }

    /// Golden snapshot (§4.10 verification standard): pins section shapes,
    /// source derivation, budget accounting and compaction provenance
    /// against silent drift. `generated_at` and the hook config paths are
    /// blanked — they are environment data (clock / tempdir), not shape.
    #[test]
    fn assembly_dump_golden_snapshot() {
        let dump = fixture_dump();
        let mut json = serde_json::to_value(&dump).unwrap();
        json["generated_at"] = serde_json::Value::String("[timestamp]".into());
        json["hooks"]["user_config_path"] = serde_json::Value::String("[user-hooks-config]".into());
        json["hooks"]["project_config_path"] =
            serde_json::Value::String("[project-hooks-config]".into());
        insta::assert_json_snapshot!("assembly_dump_golden", json);
    }
}
