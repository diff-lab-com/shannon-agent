//! Plugin permission enforcement — wires `manifest.permissions` into the
//! Shannon-side execution points (gap P7 / master plan §4.9).
//!
//! # Semantics: a declaration IS the allow-set
//!
//! `permissions` in `plugin.toml` (or `.claude-plugin/plugin.json`) is the
//! exhaustive list of capability faces the plugin asks Shannon to exercise on
//! its behalf. The effect domain is strictly the **Shannon side** — what the
//! host itself performs for the extension — because everything a plugin server
//! does *inside* its own process is not observable here:
//!
//! | [`PluginPermission`] | Shannon-side execution point | Enforced |
//! |----------------------|------------------------------|----------|
//! | `execute_commands`   | spawning the plugin stdio server (discovery + per-call cold spawn) | yes |
//! | `network`            | opening the plugin remote transport (HTTP discovery + per-call POST) | yes |
//! | `mcp_tools`          | routing calls to `mcp__<plugin>__*` tools (host tool pipeline) | yes, via [`PluginToolPolicies`] on the tool registry |
//! | `read_files`         | host reading the plugin entry/template file (command + skill extensions) | yes, [`PluginPermissionPolicy::admit_entry_read`] |
//! | `llm_api`            | prompt-based extensions driving model turns | yes, [`admit_prompt_based_extension`] |
//! | `write_files`        | the plugin's stdio server processes run inside a manifest-derived execution world (Linux Landlock fork-init / macOS Seatbelt bridge) | yes, [`PluginPermissionPolicy::spawn_sandbox_policy`] + [`crate::plugin::spawn_sandbox::PluginSpawnGuard`] |
//!
//! A manifest that **omits** `permissions` entirely deserializes to an empty
//! list, and an empty list means "nothing declared" — every point stays open,
//! preserving byte-for-byte the pre-§4.9 lenient behavior. Only a **non-empty**
//! list tightens anything: whatever was declared is allowed, everything else
//! is refused with [`PluginPermissionError`] — the single unified denial type.
//! Every gate renders its Display verbatim and emits a
//! `target = "permission/decision"` tracing event so the §4.8 event bus can
//! later carry decisions into the L0 session log without new `QueryEvent`
//! variants.
//!
//! See `PERMISSIONS.md` beside this module for the author-facing doc.

use super::manifest::{PluginManifest, PluginPermission};
use crate::mcp_tool_adapter::{DiscoveryResult, discover_tools_guarded, discover_tools_http};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

use shannon_tool_interface::sandbox::SandboxPolicy;

/// Stable prefix carried by every denial raised on behalf of a manifest.
///
/// Callers string-match this to tell a *policy* refusal apart from ordinary
/// infrastructure failures (bad script, dead socket, …).
pub const DENY_PREFIX: &str = "plugin permission denied";

/// Render a declared-permission list the way denials quote it.
fn fmt_declared(permissions: &[PluginPermission]) -> String {
    permissions
        .iter()
        .map(|p| p.wire_name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The unified error for a refused execution point.
///
/// Every gate (spawn, transport, registry routing, entry read, prompt
/// admission) funnels through this one type; downstream channels embed its
/// [`Display`](std::fmt::Display) output verbatim so denials look identical
/// across layers and always name the plugin plus its declared set.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub struct PluginPermissionError {
    /// Plugin whose manifest was consulted.
    pub plugin: String,
    /// Permission face that was requested but not granted.
    pub required: PluginPermission,
    /// The full allow-set declared by the manifest (empty = undeclared,
    /// which never produces this error).
    pub declared: Vec<PluginPermission>,
}

impl std::fmt::Display for PluginPermissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{DENY_PREFIX}: plugin '{}' is not granted '{}' — manifest declares [{}] (declaration = allow-set; add '{}' to the plugin.toml permissions)",
            self.plugin,
            self.required.wire_name(),
            fmt_declared(&self.declared),
            self.required.wire_name()
        )
    }
}

/// Denial/allowance outcome used for decision logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// The execution point proceeded.
    Allowed,
    /// The execution point was refused by the manifest allow-set.
    Denied,
}

/// Emit the per-decision trace event.
///
/// All gates report here with a stable target (`permission/decision`).
/// Since §4.8 each decision is additionally forwarded to the bus world via
/// [`crate::bus::broadcast_plugin_decision`] so both decision sources
/// (permission manager and plugin gates) persist into L0 with one schema
/// ([`SessionEventKind::PermissionDecision`](shannon_types::session_event::SessionEventKind)).
pub fn emit_decision(
    plugin: &str,
    required: PluginPermission,
    decision: PermissionDecision,
    point: &str,
    declared: &[PluginPermission],
) {
    crate::bus::broadcast_plugin_decision(crate::bus::PluginDecisionFrame {
        plugin: plugin.to_string(),
        required: required.wire_name().to_string(),
        declared: declared.iter().map(|p| p.wire_name().to_string()).collect(),
        point: point.to_string(),
        allowed: decision == PermissionDecision::Allowed,
    });
    let declared = fmt_declared(declared);
    match decision {
        PermissionDecision::Allowed => tracing::debug!(
            target: "permission/decision",
            plugin = %plugin,
            point = %point,
            permission = %required.wire_name(),
            declared = %declared,
            decision = "allow",
            "plugin permission allowed"
        ),
        PermissionDecision::Denied => tracing::warn!(
            target: "permission/decision",
            plugin = %plugin,
            point = %point,
            permission = %required.wire_name(),
            declared = %declared,
            decision = "deny",
            "plugin permission denied"
        ),
    }
}

/// The policy object derived from one manifest's `permissions` list.
///
/// Empty (undeclared) policies allow everything — see the module docs for why
/// that default is load-bearing compatibility rather than a bug.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginPermissionPolicy {
    permissions: Vec<PluginPermission>,
}

impl PluginPermissionPolicy {
    /// Policy for a manifest without a `permissions` key: allow-all.
    pub fn unspecified() -> Self {
        Self::default()
    }

    /// Build the policy from an explicit declaration list.
    pub fn from_permissions(permissions: Vec<PluginPermission>) -> Self {
        Self { permissions }
    }

    /// Derive the policy from a parsed manifest.
    pub fn from_manifest(manifest: &PluginManifest) -> Self {
        Self {
            permissions: manifest.permissions.clone(),
        }
    }

    /// True when the manifest declared nothing (default-allow mode).
    pub fn is_unspecified(&self) -> bool {
        self.permissions.is_empty()
    }

    /// The declared allow-set.
    pub fn permissions(&self) -> &[PluginPermission] {
        &self.permissions
    }

    /// Is `required` inside the allow-set?
    ///
    /// An unspecified policy answers yes to everything; otherwise exact
    /// membership decides. This is the whole of the enforcement semantics.
    pub fn allows(&self, required: PluginPermission) -> bool {
        self.permissions.is_empty() || self.permissions.contains(&required)
    }

    /// `allows`, expressed as a denial carrying plugin attribution.
    pub fn check(
        &self,
        plugin: &str,
        required: PluginPermission,
    ) -> Result<(), PluginPermissionError> {
        if self.allows(required) {
            Ok(())
        } else {
            Err(PluginPermissionError {
                plugin: plugin.to_string(),
                required,
                declared: self.permissions.clone(),
            })
        }
    }

    /// Admission of host-side reads performed for prompt-based extensions:
    /// loading the entry/template file of a command or skill plugin.
    pub fn admit_entry_read(&self, plugin: &str) -> Result<(), PluginPermissionError> {
        self.check(plugin, PluginPermission::ReadFiles)
    }

    /// Derive the **spawn-sandbox policy** this manifest imposes on its own
    /// stdio server processes: the write_files face closes the loop by
    /// turning the declaration into an OS-enforced execution world
    /// ("declaration IS sandbox") instead of an unobservable in-process
    /// promise.
    ///
    /// Derivation:
    ///
    /// | trigger | result |
    /// |---------|--------|
    /// | `write_files` **declared** | `Some(policy)` — writable roots converge to the plugin install dir + the current workspace, everything else stays read-only, system binary roots stay executable, network follows the `network` declaration |
    /// | anything else (including the *unspecified* allow-all default) | `None` — no boundary is built and the spawn chain stays byte-for-byte legacy |
    ///
    /// The membership test (not [`Self::allows_file_writes`]) is load-bearing
    /// compatibility: an *unspecified* policy answers `allows` yes to
    /// everything, and injecting a sandbox for every legacy plugin would
    /// contradict the "undeclared = unchanged" contract.
    ///
    /// Paths are canonicalized best-effort so the kernel-facing grant table
    /// and the user-space policy math agree on the same directories.
    pub fn spawn_sandbox_policy(
        &self,
        install_dir: &Path,
        workspace: &Path,
    ) -> Option<SandboxPolicy> {
        if !self.permissions.contains(&PluginPermission::WriteFiles) {
            return None;
        }
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        Some(SandboxPolicy {
            writable_roots: vec![canonical(install_dir), canonical(workspace)],
            // Everything outside the writable pair stays readable: the server
            // must still load its own interpreter, dynamic libraries and the
            // Shannon-provided script — writes are the declared capability.
            readable_roots: vec![PathBuf::from("/")],
            // System binary dirs, mirroring `seed_policy`'s defaults so
            // `sh`, the dynamic loader and common tools keep running.
            executable_roots: ["/usr", "/bin", "/sbin", "/lib", "/lib64"]
                .iter()
                .map(PathBuf::from)
                .collect(),
            network: self.permissions.contains(&PluginPermission::Network),
        })
    }

    /// Grant status of the write face.
    ///
    /// Historical scaffolding predicate (§4.9): kept because the name is
    /// pinned by tests and doc references, but note the semantic split —
    /// * enforcement consults [`Self::spawn_sandbox_policy`], which answers
    ///   `None` for unspecified policies so the legacy default stays
    ///   passthrough;
    /// * this predicate still answers via the allow-all branch for
    ///   unspecified policies (`true`), matching the general `allows`
    ///   contract.
    pub fn allows_file_writes(&self) -> bool {
        self.allows(PluginPermission::WriteFiles)
    }
}

/// Admission sequence for prompt-driven extensions (command + skill plugins).
///
/// Two Shannon-side faces fire when such an extension registers: the host
/// reads the entry file (`read_files`) and the resulting prompt drives model
/// turns (`llm_api`). Either refusal refuses registration as a whole; the
/// first failing check wins so each denial stays attributable to one field.
pub fn admit_prompt_based_extension(
    policy: &PluginPermissionPolicy,
    plugin: &str,
) -> Result<(), PluginPermissionError> {
    policy.admit_entry_read(plugin)?;
    policy.check(plugin, PluginPermission::LlmApi)
}

/// Spawn a plugin stdio server and discover its tools, gated on
/// `execute_commands`.
///
/// On success every returned adapter carries a policy clone so per-call cold
/// spawns re-check the same declaration. Errors are unified strings: denials
/// render [`PluginPermissionError`] (recognizable via [`DENY_PREFIX`]),
/// everything else is the regular discovery failure text.
///
/// This is the no-boundary form: the spawn chain is untouched. Plugins whose
/// manifest declares `write_files` go through
/// [`gated_discover_tools_stdio_guarded`], which additionally installs the
/// manifest-derived execution world around every spawn (discovery included).
pub async fn gated_discover_tools_stdio(
    policy: &Arc<PluginPermissionPolicy>,
    plugin_name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
    timeout_secs: Option<u64>,
) -> Result<DiscoveryResult, String> {
    gated_discover_tools_stdio_guarded(policy, plugin_name, command, args, env, timeout_secs, None)
        .await
}

/// [`gated_discover_tools_stdio`] with an optional execution-world boundary
/// (§ write_files enforcement, "declaration IS sandbox").
///
/// When `spawn_guard` is `Some`, **every** stdio spawn of this plugin — the
/// discovery spawn here and each adapter's per-call cold spawn — runs inside
/// the boundary; a failed fork-time install aborts the spawn (fail-closed).
/// `None` keeps the exact legacy path (zero overhead, no `pre_exec` hook),
/// which is what every non-`write_files` manifest gets.
pub async fn gated_discover_tools_stdio_guarded(
    policy: &Arc<PluginPermissionPolicy>,
    plugin_name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
    timeout_secs: Option<u64>,
    spawn_guard: Option<Arc<crate::plugin::spawn_sandbox::PluginSpawnGuard>>,
) -> Result<DiscoveryResult, String> {
    policy
        .check(plugin_name, PluginPermission::ExecuteCommands)
        .map_err(|e| {
            emit_decision(
                plugin_name,
                PluginPermission::ExecuteCommands,
                PermissionDecision::Denied,
                "stdio server spawn (discovery)",
                policy.permissions(),
            );
            e.to_string()
        })?;
    emit_decision(
        plugin_name,
        PluginPermission::ExecuteCommands,
        PermissionDecision::Allowed,
        "stdio server spawn (discovery)",
        policy.permissions(),
    );

    let mut result =
        discover_tools_guarded(plugin_name, command, args, env, timeout_secs, spawn_guard).await?;
    for tool in &mut result.tools {
        tool.set_policy(Arc::clone(policy));
    }
    Ok(result)
}

/// Open a plugin remote HTTP/SSE transport and discover its tools, gated on
/// `network`. Same contract as [`gated_discover_tools_stdio`].
pub async fn gated_discover_tools_http(
    policy: &Arc<PluginPermissionPolicy>,
    plugin_name: &str,
    url: &str,
    headers: HashMap<String, String>,
) -> Result<DiscoveryResult, String> {
    policy
        .check(plugin_name, PluginPermission::Network)
        .map_err(|e| {
            emit_decision(
                plugin_name,
                PluginPermission::Network,
                PermissionDecision::Denied,
                "remote transport connect (discovery)",
                policy.permissions(),
            );
            e.to_string()
        })?;
    emit_decision(
        plugin_name,
        PluginPermission::Network,
        PermissionDecision::Allowed,
        "remote transport connect (discovery)",
        policy.permissions(),
    );

    let mut result = discover_tools_http(plugin_name, url, &headers, None).await?;
    for tool in &mut result.tools {
        tool.set_policy(Arc::clone(policy));
    }
    Ok(result)
}

/// Extract the owning plugin from a registry tool name shaped
/// `mcp__<plugin>__<tool>`. Returns `None` for names outside that shape.
pub fn owner_of_tool(tool_name: &str) -> Option<&str> {
    let rest = tool_name.strip_prefix("mcp__")?;
    let sep = rest.find("__")?;
    let owner = &rest[..sep];
    if owner.is_empty() { None } else { Some(owner) }
}

/// Registry-side index of per-plugin policies keyed by namespace owner.
///
/// Attached onto the [`crate::tools::ToolRegistry`] during plugin loading;
/// `ToolRegistry::execute` consults it so any invocation of an
/// `mcp__<owner>__*` tool is checked against that owner's `mcp_tools` grant.
/// The production query engine, the streaming scheduler, and the
/// `ToolExecutionService` facade all funnel through those methods, so one
/// lookup covers every consumer without touching the permission core.
#[derive(Debug, Clone, Default)]
pub struct PluginToolPolicies {
    owners: HashMap<String, Arc<PluginPermissionPolicy>>,
}

impl PluginToolPolicies {
    /// Empty index — behaves as "no plugin namespaces, nothing to enforce".
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the policy governing an owner's tool namespace.
    pub fn attach(&mut self, owner: &str, policy: Arc<PluginPermissionPolicy>) {
        self.owners.insert(owner.to_string(), policy);
    }

    /// True when no plugin namespace carries a policy.
    pub fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }

    /// Resolve the owning policy for a registry tool name, if governed.
    pub fn policy_for_tool<'a>(
        &'a self,
        tool_name: &str,
    ) -> Option<(&'a str, &'a Arc<PluginPermissionPolicy>)> {
        // Owner names come from the index itself (`get_key_value`), so the
        // borrowed label outlives the temporary `tool_name` slice.
        let owner = owner_of_tool(tool_name)?;
        let (key, policy) = self.owners.get_key_value(owner)?;
        Some((key.as_str(), policy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(list: &[&str]) -> PluginPermissionPolicy {
        let perms: Vec<PluginPermission> = list
            .iter()
            .map(|n| serde_json::from_str(&format!("\"{n}\"")).expect("known permission"))
            .collect();
        PluginPermissionPolicy::from_permissions(perms)
    }

    /// Unspecified (empty) policies allow every face — the compat default.
    #[test]
    fn unspecified_policy_allows_everything() {
        let unspecified = PluginPermissionPolicy::unspecified();
        assert!(unspecified.is_unspecified());
        for required in [
            PluginPermission::ReadFiles,
            PluginPermission::WriteFiles,
            PluginPermission::ExecuteCommands,
            PluginPermission::Network,
            PluginPermission::McpTools,
            PluginPermission::LlmApi,
        ] {
            assert!(
                unspecified.allows(required),
                "undeclared manifests must stay allow-all"
            );
            assert!(unspecified.check("any-plugin", required).is_ok());
        }
    }

    /// Explicit declarations behave as an exhaustive allow-set: declared
    /// passes, everything else refuses — exercised per permission field.
    #[test]
    fn declared_set_is_exhaustive_allow_list() {
        let cases = [
            ("read_files", PluginPermission::ReadFiles),
            ("write_files", PluginPermission::WriteFiles),
            ("execute_commands", PluginPermission::ExecuteCommands),
            ("network", PluginPermission::Network),
            ("mcp_tools", PluginPermission::McpTools),
            ("llm_api", PluginPermission::LlmApi),
        ];
        for (wire, required) in cases {
            let solo = policy(&[wire]);
            assert!(solo.allows(required), "{wire} grants itself");
            assert!(solo.check("p", required).is_ok());
            // Every *other* face is refused by a single-permission manifest.
            for (_, other) in cases.iter().filter(|(_, o)| *o != required) {
                assert!(
                    !solo.allows(*other),
                    "{wire}-only manifest must refuse {}",
                    other.wire_name()
                );
                let err = solo.check("p", *other).expect_err("must refuse");
                assert_eq!(err.plugin, "p");
                assert_eq!(err.required, *other);
            }
        }
    }

    /// The unified denial names the plugin, the missing face and the set.
    #[test]
    fn denial_display_carries_plugin_attribution() {
        let ro = policy(&["read_files"]);
        let err = ro
            .check("probe-plugin", PluginPermission::ExecuteCommands)
            .expect_err("read-only manifest refuses commands");
        let text = err.to_string();
        assert!(text.starts_with(DENY_PREFIX), "unified prefix: {text}");
        assert!(text.contains("'probe-plugin'"), "{text}");
        assert!(text.contains("execute_commands"), "{text}");
        assert!(text.contains("read_files"), "quotes declared set: {text}");
    }

    /// Manifest-derived policy matches the raw derivation, both shapes.
    #[test]
    fn from_manifest_keeps_declaration_verbatim() {
        let manifest = PluginManifest::from_toml(
            r#"
name = "p"
version = "1.0.0"
description = "d"
type = "skill"
entry = "t.md"
trigger = "/t"
template = "x"
permissions = ["llm_api", "read_files"]
"#,
        )
        .expect("parses");
        let derived = PluginPermissionPolicy::from_manifest(&manifest);
        assert!(!derived.is_unspecified());
        assert!(derived.allows(PluginPermission::LlmApi));
        assert!(derived.allows(PluginPermission::ReadFiles));
        assert!(!derived.allows(PluginPermission::Network));

        // Omitted field -> unspecified -> allow-all.
        let bare = r#"
name = "silent"
version = "0.1.0"
description = "no permissions"
type = "skill"
entry = "t.md"
trigger = "/s"
template = "hi"
"#;
        let silent = PluginManifest::from_toml(bare).expect("parses");
        let silent_policy = PluginPermissionPolicy::from_manifest(&silent);
        assert!(silent_policy.is_unspecified());
        assert!(silent_policy.allows(PluginPermission::ExecuteCommands));
    }

    /// Prompt-extension admission: entry read first, then model driving;
    /// each missing field produces its own attributable denial.
    #[test]
    fn prompt_admission_checks_each_face_independently() {
        let full = policy(&["read_files", "llm_api"]);
        assert!(admit_prompt_based_extension(&full, "p").is_ok());

        let no_llm = policy(&["read_files"]);
        let err = admit_prompt_based_extension(&no_llm, "p").expect_err("missing llm_api");
        assert_eq!(err.required, PluginPermission::LlmApi);

        let no_read = policy(&["llm_api"]);
        let err = admit_prompt_based_extension(&no_read, "p").expect_err("missing read_files");
        assert_eq!(err.required, PluginPermission::ReadFiles);
    }

    /// Owner extraction handles well-formed names and rejects strangers.
    #[test]
    fn owner_of_tool_parses_namespace_shape() {
        assert_eq!(owner_of_tool("mcp__probe__read"), Some("probe"));
        assert_eq!(owner_of_tool("mcp__my.plug__deep_call"), Some("my.plug"));
        assert_eq!(owner_of_tool("Bash"), None);
        assert_eq!(owner_of_tool("mcp____x"), None);
        assert_eq!(owner_of_tool("mcp__lonely"), None);
    }

    /// Registry index resolves only governed namespaces.
    #[test]
    fn policy_index_scopes_to_attached_owner() {
        let mut index = PluginToolPolicies::new();
        assert!(index.is_empty());
        assert!(index.policy_for_tool("mcp__ghost__x").is_none());

        index.attach("probe", Arc::new(policy(&["mcp_tools"])));
        assert!(!index.is_empty());

        let (owner, resolved) = index
            .policy_for_tool("mcp__probe__anything")
            .expect("attached owner resolves");
        assert_eq!(owner, "probe");
        assert!(resolved.allows(PluginPermission::McpTools));
        assert!(!resolved.allows(PluginPermission::Network));
        assert!(index.policy_for_tool("Other").is_none());
    }

    /// Reserved write face reports grant status; an unspecified policy still
    /// answers true through the allow-all branch, never via membership.
    #[test]
    fn write_files_scaffolding_reports_but_never_throws() {
        let with_write = policy(&["write_files"]);
        assert!(with_write.allows_file_writes());
        let empty_only = policy(&[]);
        assert!(empty_only.is_unspecified());
        assert!(empty_only.allows_file_writes());
        let read_only = policy(&["read_files"]);
        assert!(!read_only.allows_file_writes());
    }

    // ── write_files enforcement: spawn-sandbox derivation ──────────────

    use std::path::Path;

    /// Only a *declared* write_files face derives a spawn-sandbox policy —
    /// and the derivation scopes writes to install dir + workspace, keeps
    /// the rest read-only, and follows the network declaration. The
    /// unspecified policy derives `None`: that membership-vs-allow-all
    /// distinction IS the default-allow compat contract.
    #[test]
    fn spawn_sandbox_derivation_scopes_write_files_declaration() {
        let dir = tempfile::tempdir().expect("temp dir");
        let install = dir.path().join("plugins").join("probe");
        std::fs::create_dir_all(&install).expect("install dir");
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(&workspace).expect("workspace dir");

        // Compat red line: unspecified manifests never gain a boundary.
        assert!(
            policy(&[])
                .spawn_sandbox_policy(&install, &workspace)
                .is_none()
        );
        // Declared faces without write_files stay passthrough too.
        assert!(
            policy(&["read_files", "mcp_tools"])
                .spawn_sandbox_policy(&install, &workspace)
                .is_none()
        );

        let derived = policy(&["write_files"])
            .spawn_sandbox_policy(&install, &workspace)
            .expect("write_files derives a policy");
        // Paths canonicalized (dir has no symlinks here, but resolve the
        // tempdir's own /tmp alias on macOS before comparing).
        let install_c = std::fs::canonicalize(&install).expect("install");
        let workspace_c = std::fs::canonicalize(&workspace).expect("workspace");
        assert_eq!(derived.writable_roots, vec![install_c, workspace_c]);
        assert_eq!(derived.readable_roots, vec![Path::new("/")]);
        assert!(
            !derived.network,
            "network undeclared ⇒ child world has none"
        );
        for root in ["/usr", "/bin", "/sbin", "/lib", "/lib64"] {
            assert!(
                derived
                    .executable_roots
                    .contains(&std::path::PathBuf::from(root)),
                "system binary root {root} must stay executable"
            );
        }

        // Declared network propagates into the child world.
        let networked = policy(&["write_files", "network"])
            .spawn_sandbox_policy(&install, &workspace)
            .expect("derives");
        assert!(networked.network, "network follows its declaration");
    }

    /// The derivation canonicalizes what it can and falls back verbatim for
    /// unresolvable inputs — production roots are absolute (home-relative
    /// plugins dir + process CWD), and the kernel boundary (`PathFd` at fork)
    /// is the fail-closed backstop for any unusable root, never a silent
    /// re-grant. The readable root stays the absolute `/` either way.
    #[test]
    fn spawn_sandbox_derivation_canonicalizes_or_falls_back_verbatim() {
        let derived = policy(&["write_files"])
            .spawn_sandbox_policy(Path::new("definitely/missing/dir"), Path::new("."))
            .expect("derives");
        assert_eq!(
            derived.writable_roots[0],
            Path::new("definitely/missing/dir"),
            "unresolvable input is kept verbatim, not silently re-rooted"
        );
        assert_eq!(derived.readable_roots, vec![Path::new("/")]);
    }
}
