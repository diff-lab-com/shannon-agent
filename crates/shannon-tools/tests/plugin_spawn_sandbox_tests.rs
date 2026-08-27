//! write_files enforcement ("declaration IS sandbox") — e2e acceptance.
//!
//! Pins the closing of the §4.9 scaffolding (gap P7 tail): a plugin whose
//! manifest declares `write_files` gets its stdio server processes — the
//! discovery spawn **and** every per-call cold spawn — born inside a
//! manifest-derived execution world:
//!
//! - writable roots converge to the plugin install dir + the current
//!   workspace; everything else stays read-only;
//! - writes outside those roots are refused by the **kernel** (Landlock),
//!   observable as failed writes inside the plugin's own subprocess;
//! - manifests *without* the declaration spawn with no boundary at all —
//!   byte-for-byte legacy behavior (the default-allow compat red line).
//!
//! Landlock cells execute only on hosts that actually enforce Landlock;
//! otherwise they print the probe reason and return early (the labeled-skip
//! convention from `sandbox_matrix.rs`).

use serde_json::json;
use shannon_core::mcp_tool_adapter::McpToolAdapter;
use shannon_core::plugin::{
    PluginManifest, PluginPermissionPolicy, gated_discover_tools_stdio,
    gated_discover_tools_stdio_guarded,
};
use shannon_tool_interface::sandbox::SandboxError;
use shannon_tool_interface::{Tool, ToolOutput};
use shannon_tools::sandbox::plugin_spawn_world;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

const PLUGIN_NAME: &str = "sandbox-probe";
const TOOL_WRITE_INSIDE: &str = "mcp__sandbox-probe__write_inside";
const TOOL_WRITE_OUTSIDE: &str = "mcp__sandbox-probe__write_outside";

/// stdio server spawned exactly like a Shannon tool plugin. On `tools/call`
/// it attempts one file write and reports the *observed outcome* in the MCP
/// result text — under Landlock the refusal comes from the kernel inside the
/// child, so the report is the enforcement signal.
///
/// No `/dev/null` redirects: `/dev` is not a granted root, and a failed
/// redirection would flip the branch regardless of the write's own outcome.
const FAKE_SERVER_SH: &str = r#"#!/bin/sh
echo spawned >> "$PROBE_SPAWN_MARKER"
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"sandbox-probe","version":"1.0.0"}}}'
      ;;
    *'"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"write_inside"},{"name":"write_outside"}]}}'
      ;;
    *'"prompts/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"prompts":[]}}'
      ;;
    *'"tools/call"'*)
      case "$line" in
        *write_inside*)
          if printf 'inside-ok' > "$PROBE_INSIDE_TARGET"; then
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"inside-write:ok"}]}}'
          else
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"inside-write:denied"}]}}'
          fi
          ;;
        *write_outside*)
          if printf 'pwn-by-plugin' > "$PROBE_OUTSIDE_TARGET"; then
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"outside-write:ok"}]}}'
          else
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"outside-write:denied"}]}}'
          fi
          ;;
      esac
      ;;
  esac
done
"#;

struct Probe {
    /// Workspace (the sandbox's second writable root).
    workspace: TempDir,
    /// A directory deliberately OUTSIDE every granted root.
    outside: TempDir,
    script: PathBuf,
    spawn_marker: PathBuf,
    inside_target: PathBuf,
    outside_target: PathBuf,
}

impl Probe {
    fn new() -> Self {
        let workspace = TempDir::new().expect("workspace temp dir");
        let outside = TempDir::new().expect("outside temp dir");
        let script = workspace.path().join("probe_server.sh");
        fs::write(&script, FAKE_SERVER_SH).expect("write fake server script");

        let spawn_marker = workspace.path().join("spawn_markers.txt");
        let inside_target = workspace.path().join("inside_written.txt");
        let outside_target = outside.path().join("outside_target.txt");

        Self {
            workspace,
            outside,
            script,
            spawn_marker,
            inside_target,
            outside_target,
        }
    }

    fn install_dir(&self) -> PathBuf {
        // The canonical plugin layout: <workspace>/plugins/<name>/plugin.toml
        let dir = self.workspace.path().join("plugins").join(PLUGIN_NAME);
        fs::create_dir_all(&dir).expect("plugin install dir");
        dir
    }

    fn env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        for (key, path) in [
            ("PROBE_SPAWN_MARKER", &self.spawn_marker),
            ("PROBE_INSIDE_TARGET", &self.inside_target),
            ("PROBE_OUTSIDE_TARGET", &self.outside_target),
        ] {
            env.insert(
                key.to_string(),
                path.to_str().expect("utf-8 path").to_string(),
            );
        }
        env
    }

    fn spawn_count(&self) -> usize {
        fs::read_to_string(&self.spawn_marker)
            .expect("spawn marker recorded")
            .lines()
            .count()
    }
}

fn take_tool(tools: &mut Vec<McpToolAdapter>, wanted: &str) -> McpToolAdapter {
    let index = tools
        .iter()
        .position(|t| t.registry_name() == wanted)
        .unwrap_or_else(|| panic!("discovered tools contain {wanted}"));
    tools.remove(index)
}

fn result_text(output: ToolOutput) -> String {
    assert!(!output.is_error, "call must complete: {}", output.content);
    output.content
}

/// Declared `write_files`: the whole spawn chain (discovery + both per-call
/// cold spawns) runs sandboxed — workspace writes succeed, writes outside the
/// declared roots are refused by the kernel, and the refusal never crashes
/// the server (it reports and continues).
#[tokio::test]
async fn writefiles_plugin_writes_are_kernel_scoped_to_declared_roots() {
    let probe = Probe::new();

    let manifest_body = format!(
        r#"
name = "{PLUGIN_NAME}"
version = "1.0.0"
description = "write_files sandbox probe"
type = "tool"
entry = "server.sh"
permissions = ["write_files", "execute_commands", "mcp_tools"]

[transport]
type = "stdio"
command = "sh"
args = ["{}"]
"#,
        probe.script.to_str().expect("utf-8 script path"),
    );
    let manifest = PluginManifest::from_toml(&manifest_body).expect("manifest parses");
    let policy = Arc::new(PluginPermissionPolicy::from_manifest(&manifest));

    let install_dir = probe.install_dir();
    let workspace = probe.workspace.path();

    // Derive + build exactly like the production wiring helper does.
    let derived = policy
        .spawn_sandbox_policy(&install_dir, workspace)
        .expect("write_files declared ⇒ policy derived");
    let world = match plugin_spawn_world(derived, workspace) {
        Ok(world) => world,
        Err(SandboxError::Unsupported { backend, detail }) => {
            println!("skip: host cannot enforce '{backend}': {detail}");
            return;
        }
        Err(other) => panic!("unexpected sandbox error: {other}"),
    };
    assert_eq!(world.guard.kind(), "landlock");
    assert_eq!(
        world.policy.writable_roots.len(),
        2,
        "install dir + workspace, nothing else"
    );
    assert!(
        !world.policy.network,
        "network undeclared ⇒ child world has none"
    );

    let mut discovery = gated_discover_tools_stdio_guarded(
        &policy,
        PLUGIN_NAME,
        "sh",
        &[probe.script.to_string_lossy().into_owned()],
        &probe.env(),
        Some(15),
        Some(world.guard),
    )
    .await
    .expect("declared plugin spawns inside the sandboxed world");

    // Discovery spawn happened — inside the world (marker is in-workspace).
    assert_eq!(probe.spawn_count(), 1, "one sandboxed discovery spawn");

    let mut tools = std::mem::take(&mut discovery.tools);
    assert!(
        tools.iter().all(|t| t.spawn_guard().is_some()),
        "every adapter carries the boundary for its per-call cold spawns"
    );

    let inside = take_tool(&mut tools, TOOL_WRITE_INSIDE);
    let outside = take_tool(&mut tools, TOOL_WRITE_OUTSIDE);
    drop(tools);
    drop(discovery);

    // In-roots write: allowed and actually lands.
    let text = result_text(inside.execute(json!({})).await.expect("inside call"));
    assert!(text.contains("inside-write:ok"), "{text}");
    assert_eq!(
        fs::read_to_string(&probe.inside_target).expect("in-root write landed"),
        "inside-ok"
    );

    // Out-of-roots write: the KERNEL refuses inside the child; the server
    // reports the denial and nothing exists at the target.
    let text = result_text(outside.execute(json!({})).await.expect("outside call"));
    assert!(text.contains("outside-write:denied"), "{text}");
    assert!(
        !probe.outside_target.exists(),
        "write outside the declared roots must not land"
    );

    // Every cold spawn (discovery + 2 calls) ran the enforced chain.
    assert!(
        probe.spawn_count() >= 3,
        "discovery + per-call spawns all completed, got {}",
        probe.spawn_count()
    );
}

/// Compatibility red line: an UNDECLARED manifest gets no boundary anywhere —
/// the derivation is `None`, adapters carry no guard, and the child writes
/// outside every conceivable root successfully, exactly like before this
/// enforcement existed.
#[tokio::test]
async fn undeclared_plugin_spawn_chain_is_unchanged_byte_for_byte() {
    let probe = Probe::new();

    let manifest_body = format!(
        r#"
name = "{PLUGIN_NAME}"
version = "1.0.0"
description = "no permissions declared"
type = "tool"
entry = "server.sh"

[transport]
type = "stdio"
command = "sh"
args = ["{}"]
"#,
        probe.script.to_str().expect("utf-8 script path"),
    );
    let manifest = PluginManifest::from_toml(&manifest_body).expect("manifest parses");
    assert!(
        manifest.permissions.is_empty(),
        "fixture is truly undeclared"
    );
    let policy = Arc::new(PluginPermissionPolicy::from_manifest(&manifest));

    // No derivation, no world — the passthrough is total.
    let install_dir = probe.install_dir();
    let workspace = probe.workspace.path();
    assert!(
        policy
            .spawn_sandbox_policy(&install_dir, workspace)
            .is_none(),
        "undeclared manifests must never derive a sandbox policy"
    );

    // Legacy discovery entry point (the pre-enforcement signature).
    let mut discovery = gated_discover_tools_stdio(
        &policy,
        PLUGIN_NAME,
        "sh",
        &[probe.script.to_string_lossy().into_owned()],
        &probe.env(),
        Some(15),
    )
    .await
    .expect("undeclared manifest spawns exactly as before");
    let mut tools = std::mem::take(&mut discovery.tools);
    assert!(
        tools.iter().all(|t| t.spawn_guard().is_none()),
        "no boundary may ride along for undeclared plugins"
    );

    let inside = take_tool(&mut tools, TOOL_WRITE_INSIDE);
    let outside = take_tool(&mut tools, TOOL_WRITE_OUTSIDE);
    drop(tools);
    drop(discovery);

    let text = result_text(outside.execute(json!({})).await.expect("outside call"));
    assert!(
        text.contains("outside-write:ok"),
        "undeclared plugin child stays unrestricted (legacy): {text}"
    );
    assert_eq!(
        fs::read_to_string(&probe.outside_target).expect("legacy write lands anywhere"),
        "pwn-by-plugin"
    );

    let text = result_text(inside.execute(json!({})).await.expect("inside call"));
    assert!(text.contains("inside-write:ok"), "{text}");
}

/// Guard-rail for the skip path itself: the fixture workspace layout is sound
/// (install dir inside workspace, outside dir disjoint) so the landlock cells
/// above measure exactly what they claim.
#[test]
fn probe_layout_scopes_roots_as_documented() {
    let probe = Probe::new();
    let install = probe.install_dir();
    let workspace: &Path = probe.workspace.path();
    let outside: &Path = probe.outside.path();

    let starts_with = |p: &Path, root: &Path| p.starts_with(root);
    assert!(starts_with(&install, workspace), "install inside workspace");
    assert!(
        !starts_with(outside, workspace),
        "outside disjoint from workspace"
    );
    assert!(
        !starts_with(workspace, outside),
        "workspace disjoint from outside"
    );
}
