//! 4.5 W0-T0 — plugin manifest permission enforcement probes.
//!
//! Master plan §4.5 / gap analysis P7 (`TODO-verify`): the `permissions` list
//! on `plugin.toml` manifests ([`PluginPermission`]: read_files, write_files,
//! execute_commands, network, mcp_tools, llm_api) is suspected to be purely
//! declarative. This file settles the question end-to-end against the real
//! plugin plumbing with **zero implementation changes**:
//!
//! 1. what an *undeclared* `permissions` list means (default semantics),
//! 2. whether `PluginRegistry` load applies any manifest-based gate,
//! 3. whether tool discovery spawns an arbitrary server subprocess while the
//!    manifest declares only `read_files` (no `execute_commands`),
//! 4. whether plugin tool calls actually produce write/command side effects
//!    that were never declared,
//! 5. whether the runtime pipeline gate (`ToolExecutionService` ->
//!    `PermissionManager::check_tool_permission`) ever sees manifest data.
//!
//! The subprocess probes run a throwaway fake MCP server (`sh` script) whose
//! sole job is to leave observable markers (files) behind, mirroring exactly
//! how `repl/mod.rs` wires plugins: `PluginRegistry::load_all()` +
//! `discover_tools()` + `ToolRegistry::register`.
//!
//! Verdict encoded here is the CURRENT behavior (characterization, not spec):
//! "declared-but-unenforced" is precisely the P7 gap. When §4.9 closes P7,
//! tests 3-5 below are the ones expected to flip to rejection assertions.

use shannon_core::plugin::{PluginManifest, PluginPermission};

// ── Cross-platform: manifest-level semantics ───────────────────────────────

/// A manifest that omits `permissions` entirely parses to an **empty list**
/// and loads normally. In "permission list" semantics an empty list means the
/// plugin declares nothing yet everything downstream still proceeds (proven at
/// execution level by the unix probes below) — i.e. the default is allow-all.
#[test]
fn undeclared_permissions_default_to_an_empty_list() {
    let toml = r#"
name = "silent-plugin"
version = "0.1.0"
description = "no permissions field at all"
type = "skill"
entry = "template.md"
trigger = "/silent"
template = "hi"
"#;
    let manifest = PluginManifest::from_toml(toml).expect("manifest parses without permissions");
    assert!(
        manifest.permissions.is_empty(),
        "omitted permissions must deserialize to an empty vec, got {:?}",
        manifest.permissions
    );
}

/// Pin the exact shape of [`PluginPermission`]: the six documented variants
/// with their serde wire names. No wildcard arm — adding or removing a
/// variant must fail compilation here so the §4.5 permission matrix gets
/// revisited instead of silently drifting.
#[test]
fn permission_enum_declares_exactly_the_six_documented_permissions() {
    let declared: [(&str, PluginPermission); 6] = [
        ("read_files", PluginPermission::ReadFiles),
        ("write_files", PluginPermission::WriteFiles),
        ("execute_commands", PluginPermission::ExecuteCommands),
        ("network", PluginPermission::Network),
        ("mcp_tools", PluginPermission::McpTools),
        ("llm_api", PluginPermission::LlmApi),
    ];
    for (wire_name, permission) in &declared {
        let serialized = serde_json::to_string(permission)
            .expect("unit enum variant serializes to its wire name");
        assert_eq!(serialized, format!("\"{wire_name}\""));
    }
}

// ── Unix-only: end-to-end subprocess probes ────────────────────────────────
//
// Every test below drives the production seams used by repl/mod.rs, with the
// manifest declaring ONLY `read_files` while exercising write/exec paths.

#[cfg(unix)]
mod unix_probe {
    use serde_json::json;
    use shannon_core::discover_tools;
    use shannon_core::mcp_tool_adapter::{DiscoveryResult, McpToolAdapter};
    use shannon_core::plugin::{PluginKind, PluginRegistry};
    use shannon_core::tool_execution::{ToolExecutionError, ToolExecutionService};
    use shannon_core::tools::ToolRegistry;
    use shannon_engine::permissions::{Permission, PermissionLevel, PermissionManager};
    use shannon_tool_interface::Tool;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::TempDir;
    use uuid::Uuid;

    const PLUGIN_NAME: &str = "probe-plugin";
    const TOOL_READ: &str = "mcp__probe-plugin__probe_read";
    const TOOL_WRITE: &str = "mcp__probe-plugin__probe_write";
    const TOOL_EXEC: &str = "mcp__probe-plugin__probe_exec";

    /// Manifest declared exactly as briefed: reads allowed, writes and
    /// command execution deliberately NOT declared.
    const MANIFEST_READ_ONLY_TOML: &str = r#"
name = "probe-plugin"
version = "1.0.0"
description = "permission enforcement probe"
type = "tool"
entry = "server.sh"
permissions = ["read_files"]

[transport]
type = "stdio"
command = "sh"
args = ["__PROBE_SCRIPT__"]
"#;

    /// Throwaway MCP stdio server: answers discovery, and on `tools/call`
    /// leaves filesystem markers that prove the side effect actually ran.
    /// Any "enforcement" in front of this process cannot see these markers -
    /// which is the point: nobody intercepts them.
    const FAKE_SERVER_SH: &str = r#"#!/bin/sh
# Probe server spawned exactly like a Shannon tool plugin.
# Markers are keyed by environment vars passed by the test harness.
[ -n "$PROBE_SPAWN_MARKER" ] && echo spawn >> "$PROBE_SPAWN_MARKER"

while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in
    *'"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"probe-plugin","version":"1.0.0"}}}'
      ;;
    *'"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"probe_read"},{"name":"probe_write"},{"name":"probe_exec"}]}}'
      ;;
    *'"prompts/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"prompts":[]}}'
      ;;
    *'"tools/call"'*)
      case "$line" in
        *probe_read*)
          READ_CONTENT=$(cat "$PROBE_READ_FILE")
          printf '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"read:%s"}]}}\n' "$READ_CONTENT"
          ;;
        *probe_write*)
          printf 'pwn-by-plugin' > "$PROBE_WRITE_TARGET"
          printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"write-done"}]}}'
          ;;
        *probe_exec*)
          /bin/sh -c 'echo exec-did-run > "$1"' sh "$PROBE_EXEC_TARGET"
          printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"exec-done"}]}}'
          ;;
      esac
      ;;
  esac
done
"#;

    struct Probe {
        dir: TempDir,
        script: PathBuf,
        env: HashMap<String, String>,
        write_target: PathBuf,
        exec_target: PathBuf,
        spawn_marker: PathBuf,
    }

    impl Probe {
        fn new() -> Self {
            let dir = TempDir::new().expect("temp dir for probe fixture");
            let base = dir.path();

            let script = base.join("probe_server.sh");
            fs::write(&script, FAKE_SERVER_SH).expect("write fake server script");

            let read_source = base.join("fixture-read-ok.txt");
            fs::write(&read_source, "fixture-ok").expect("write read fixture");

            let write_target = base.join("written_by_plugin.txt");
            let exec_target = base.join("executed_by_plugin.txt");
            let spawn_marker = base.join("spawn_markers.txt");

            let mut env = HashMap::new();
            env.insert(
                "PROBE_SPAWN_MARKER".to_string(),
                spawn_marker.to_string_lossy().into_owned(),
            );
            env.insert(
                "PROBE_READ_FILE".to_string(),
                read_source.to_string_lossy().into_owned(),
            );
            env.insert(
                "PROBE_WRITE_TARGET".to_string(),
                write_target.to_string_lossy().into_owned(),
            );
            env.insert(
                "PROBE_EXEC_TARGET".to_string(),
                exec_target.to_string_lossy().into_owned(),
            );

            Self {
                dir,
                script,
                env,
                write_target,
                exec_target,
                spawn_marker,
            }
        }

        fn manifest_toml(&self) -> String {
            MANIFEST_READ_ONLY_TOML.replace(
                "__PROBE_SCRIPT__",
                self.script.to_str().expect("script path is valid UTF-8"),
            )
        }

        fn install_manifest(&self) -> PathBuf {
            let plugins_dir = self.dir.path().join("plugins").join(PLUGIN_NAME);
            fs::create_dir_all(&plugins_dir).expect("create plugin dir");
            let toml_path = plugins_dir.join("plugin.toml");
            fs::write(&toml_path, self.manifest_toml()).expect("write plugin.toml");
            plugins_dir
                .parent()
                .expect("plugins dir parent")
                .to_path_buf()
        }

        async fn discover(&self) -> DiscoveryResult {
            discover_tools(
                PLUGIN_NAME,
                "sh",
                &[self.script.to_string_lossy().into_owned()],
                &self.env,
                Some(10),
            )
            .await
            .expect("discovery succeeds end-to-end")
        }
    }

    fn take_tool(discovery: &mut DiscoveryResult, wanted: &str) -> McpToolAdapter {
        let index = discovery
            .tools
            .iter()
            .position(|t| t.registry_name() == wanted)
            .unwrap_or_else(|| panic!("discovered tools contain {wanted}"));
        discovery.tools.remove(index)
    }

    /// Load-time: a manifest declaring only `read_files` installs and enables
    /// with no complaint. There is no validation, prompt, or rejection step
    /// tied to the declaration anywhere in `PluginRegistry`.
    #[tokio::test]
    async fn registry_load_applies_no_gate_for_undeclared_permissions() {
        let probe = Probe::new();
        let plugins_dir = probe.install_manifest();

        let mut registry = PluginRegistry::new(plugins_dir);
        registry.load_all().await.expect("load_all succeeds");

        let loaded = registry.get(PLUGIN_NAME).expect("plugin was registered");
        assert!(loaded.enabled, "plugin is enabled by default");
        assert_eq!(loaded.manifest.permissions.len(), 1);

        match loaded.manifest.kind().expect("valid tool plugin") {
            PluginKind::Tool { transport } => {
                assert_eq!(transport.command(), Some("sh"));
            }
            other => panic!("expected Tool kind, got {other:?}"),
        }
    }

    /// Discovery spawns the server subprocess even though the manifest did
    /// NOT declare `execute_commands`. Arbitrary startup code ran - the spawn
    /// itself is an unconditional execution point for anything the plugin
    /// server does.
    #[tokio::test]
    async fn discovery_spawns_plugin_process_despite_execute_commands_not_declared() {
        let probe = Probe::new();
        let discovery = probe.discover().await;

        let names: Vec<&str> = discovery.tools.iter().map(|t| t.registry_name()).collect();
        assert_eq!(names.len(), 3);
        for wanted in [TOOL_READ, TOOL_WRITE, TOOL_EXEC] {
            assert!(names.contains(&wanted), "missing {wanted} in {names:?}");
        }
        assert!(discovery.prompts.is_empty());

        let markers =
            fs::read_to_string(&probe.spawn_marker).expect("server startup marker recorded");
        assert_eq!(
            markers.lines().count(),
            1,
            "exactly one discovery-time spawn happened with no gate in front of it"
        );
    }

    /// The core gap proof: declared read-only, yet the plugin tool performs a
    /// filesystem write and a shell command execution anyway. Nothing between
    /// the caller and the side effect consults `manifest.permissions`.
    ///
    /// `probe_read` doubles as the positive control proving the plumbing works
    /// when the permission IS declared - so the observations above are about
    /// enforcement being absent, not the pipeline being broken.
    #[tokio::test]
    async fn tool_calls_perform_write_and_command_side_effects_without_declarations() {
        let probe = Probe::new();
        let mut discovery = probe.discover().await;

        // Positive control: reads (the DECLARED permission) flow through the
        // very same unguarded pipeline.
        let read_adapter = take_tool(&mut discovery, TOOL_READ);
        let output = read_adapter
            .execute(json!({}))
            .await
            .expect("tool executes");
        assert!(!output.is_error);
        assert_eq!(output.content, "read:fixture-ok");

        // write_files NOT declared -> still writes.
        let write_adapter = take_tool(&mut discovery, TOOL_WRITE);
        let output = write_adapter
            .execute(json!({}))
            .await
            .expect("tool executes");
        assert!(!output.is_error);
        assert!(
            probe.write_target.exists(),
            "plugin wrote a file although write_files was never declared"
        );

        // execute_commands NOT declared -> still spawns commands.
        let exec_adapter = take_tool(&mut discovery, TOOL_EXEC);
        let output = exec_adapter
            .execute(json!({}))
            .await
            .expect("tool executes");
        assert!(!output.is_error);
        assert!(
            probe.exec_target.exists(),
            "plugin executed a command although execute_commands was never declared"
        );

        // One spawn per call: discovery + three executions, none of them gated.
        let markers = fs::read_to_string(&probe.spawn_marker).expect("spawn marker recorded");
        assert_eq!(markers.lines().count(), 4);
    }

    /// Runtime pipeline gate: `run_tool_use` consults only session-scoped,
    /// name-keyed rules (`PermissionManager`). Two decisions below use the
    /// SAME manager/registry wiring where one plugin tool is blocked by an
    /// explicit name rule while its undeclared-write sibling sails through -
    /// manifest data never reaches the decision either way.
    #[tokio::test]
    async fn execution_pipeline_decisions_are_name_keyed_and_manifest_blind() {
        let probe = Probe::new();
        let mut discovery = probe.discover().await;

        let write_adapter = take_tool(&mut discovery, TOOL_WRITE);
        let read_adapter = take_tool(&mut discovery, TOOL_READ);

        let registry = Arc::new(ToolRegistry::new());
        registry
            .register(Box::new(write_adapter))
            .expect("probe_write registers");
        registry
            .register(Box::new(read_adapter))
            .expect("probe_read registers");

        // Default manager: no configured requirement for this plugin's names,
        // so execution proceeds regardless of what plugin.toml did/did not
        // declare (empty-declaration default is allow-all).
        let manager = Arc::new(PermissionManager::new());
        let service = ToolExecutionService::new(Arc::clone(&registry), Arc::clone(&manager));
        service
            .run_tool_use(Uuid::new_v4(), TOOL_WRITE, json!({}))
            .await
            .expect("undeclared write passes the pipeline gate untouched");
        assert!(probe.write_target.exists());

        // Now block ONLY probe_read by explicit name rule; the undeclared
        // write tool keeps running under the identical configuration. If the
        // gate saw the manifest, both would be judged differently.
        let mut strict_manager = PermissionManager::new();
        strict_manager.set_tool_permission(
            TOOL_READ.to_string(),
            Permission::new("mcp_tools", "execute", PermissionLevel::Read),
        );
        let strict_service =
            ToolExecutionService::new(Arc::clone(&registry), Arc::new(strict_manager));

        let denied = strict_service
            .run_tool_use(Uuid::new_v4(), TOOL_READ, json!({}))
            .await;
        match denied {
            Err(ToolExecutionError::PermissionDenied { tool_name, .. }) => {
                assert_eq!(tool_name, TOOL_READ);
            }
            other => panic!("expected PermissionDenied for {TOOL_READ}, got {other:?}"),
        }

        strict_service
            .run_tool_use(Uuid::new_v4(), TOOL_WRITE, json!({}))
            .await
            .expect("manifest-blind gate stays open for the undeclared write");
    }
}
