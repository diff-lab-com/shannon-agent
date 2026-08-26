//! §4.9 W0 — plugin manifest permission enforcement regression suite.
//!
//! Permanent successor of the §4.5 characterization probes (commit `5f83afc6`
//! proved every `PluginPermission` face was unenforced, closing the
//! investigation half of gap P7). This file pins the *enforcement* semantics
//! shipped by §4.9 at each Shannon-side execution point.
//!
//! Matrix (permission -> Shannon-side point -> enforced by):
//!
//! | permission        | execution point (host side)                          | enforcement |
//! |-------------------|------------------------------------------------------|-------------|
//! | execute_commands  | plugin stdio server spawn: discovery + per-call      | gated_discover_tools_stdio + adapter spawn gate |
//! | network           | plugin remote transport connect: discovery + per-call| gated_discover_tools_http + adapter transport gate |
//! | mcp_tools         | routing calls to `mcp__<plugin>__*` tools            | ToolRegistry policy index (`attach_plugin_policy`) |
//! | read_files        | host reading a prompt extension's entry file         | admit_prompt_based_extension (repl/cli registration) |
//! | llm_api           | prompt-driven extensions driving model turns         | admit_prompt_based_extension |
//! | write_files       | reserved — no post-manifest host-side write face yet | scaffolding (`allows_file_writes`), lands with §4.11 FileSystemProvider seam |
//!
//! Every runtime-observable field carries positive (declared -> allowed) and
//! negative (undeclared -> refused) cases below; read_files/llm_api positives
//! and negatives live in `permissions.rs` unit tests plus the registration
//! sequence test here. A dedicated compatibility group proves an *undeclared*
//! manifest behaves byte-for-byte like pre-§4.9 (load, spawn, call).
//!
//! Semantics reference: `crates/shannon-core/src/plugin/PERMISSIONS.md`.

use shannon_core::plugin::{PluginManifest, PluginPermission, PluginPermissionPolicy};

// ── Cross-platform: manifest-level semantics ───────────────────────────────

/// A manifest that omits `permissions` entirely parses to an **empty list**
/// and loads normally. Empty = undeclared = allow-all at every execution
/// point (proven end-to-end by the unix compat group below).
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
    let policy = PluginPermissionPolicy::from_manifest(&manifest);
    assert!(policy.is_unspecified());
}

/// Pin the exact shape of [`PluginPermission`]: the six documented variants
/// with their serde wire names. No wildcard arm — adding or removing a
/// variant must fail compilation here so the permission matrix gets revisited
/// instead of silently drifting.
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

#[cfg(unix)]
mod unix_probe {
    use serde_json::json;
    use shannon_core::mcp_tool_adapter::{DiscoveryResult, McpToolAdapter};
    use shannon_core::plugin::{
        DENY_PREFIX, PluginManifest, PluginPermission, PluginPermissionPolicy,
        admit_prompt_based_extension, gated_discover_tools_http, gated_discover_tools_stdio,
        owner_of_tool,
    };
    use shannon_core::tool_execution::{ToolExecutionError, ToolExecutionService};
    use shannon_core::tools::ToolRegistry;
    use shannon_engine::permissions::PermissionManager;
    use shannon_tool_interface::{Tool, ToolOutput, ToolResult};
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::TempDir;
    use uuid::Uuid;

    struct EchoTool(String);

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            &self.0
        }
        fn description(&self) -> &str {
            "ungoverned echo"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object"})
        }
        async fn execute(&self, _: serde_json::Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::success("echo-ok".to_string()))
        }
    }

    const PLUGIN_NAME: &str = "probe-plugin";
    const TOOL_READ: &str = "mcp__probe-plugin__probe_read";
    const TOOL_WRITE: &str = "mcp__probe-plugin__probe_write";
    const TOOL_EXEC: &str = "mcp__probe-plugin__probe_exec";

    /// Manifest template. `__PERMISSIONS__` is replaced with either a
    /// `permissions = [...]` line or nothing (the compatibility shape).
    const MANIFEST_TEMPLATE_TOML: &str = r#"
name = "probe-plugin"
version = "1.0.0"
description = "permission enforcement probe"
type = "tool"
entry = "server.sh"
__PERMISSIONS__

[transport]
type = "stdio"
command = "sh"
args = ["__PROBE_SCRIPT__"]
"#;

    /// Throwaway MCP stdio server: answers discovery, and on `tools/call`
    /// leaves filesystem markers that prove the side effect actually ran.
    const FAKE_SERVER_SH: &str = r#"#!/bin/sh
# Probe server spawned exactly like a Shannon tool plugin.
# Markers are keyed by environment vars passed by the test harness.
echo $PROBE_SPAWN_MARKER >> "$PROBE_SPAWN_MARKER"
while IFS= read -r line; do
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

    fn manifest_with(perms_line: &str) -> String {
        MANIFEST_TEMPLATE_TOML.replace("__PERMISSIONS__\n", perms_line)
    }

    /// The six grants everything needs — the canonical fully-declared tool
    /// plugin used as the positive control.
    const FULL_PERMISSIONS_LINE: &str = r#"permissions = ["read_files", "write_files", "execute_commands", "network", "mcp_tools"]"#;

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

        /// Write plugin.toml into a `<dir>/plugins/<name>/` layout and return
        /// the `plugins/` directory (what [`PluginRegistry`] scans).
        fn install_manifest(&self, toml_body: &str) -> PathBuf {
            let plugin_dir = self.dir.path().join("plugins").join(PLUGIN_NAME);
            fs::create_dir_all(&plugin_dir).expect("create plugin dir");
            let toml_path = plugin_dir.join("plugin.toml");
            let rendered = toml_body.replace(
                "__PROBE_SCRIPT__",
                self.script.to_str().expect("script path is valid UTF-8"),
            );
            fs::write(&toml_path, rendered).expect("write plugin.toml");
            self.dir.path().join("plugins")
        }

        fn parse_manifest(&self, toml_body: &str) -> PluginManifest {
            PluginManifest::from_toml(toml_body).expect("probe manifest parses")
        }

        async fn discover_gated(
            &self,
            policy: &Arc<PluginPermissionPolicy>,
        ) -> Result<DiscoveryResult, String> {
            gated_discover_tools_stdio(
                policy,
                PLUGIN_NAME,
                "sh",
                &[self.script.to_string_lossy().into_owned()],
                &self.env,
                Some(10),
            )
            .await
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

    fn spawn_count(probe: &Probe) -> usize {
        fs::read_to_string(&probe.spawn_marker)
            .expect("spawn marker recorded")
            .lines()
            .count()
    }

    fn service_for(registry: &Arc<ToolRegistry>) -> ToolExecutionService {
        ToolExecutionService::new(Arc::clone(registry), Arc::new(PermissionManager::new()))
    }

    // ── Compatibility group: UNDECLARED manifests behave exactly as before ──

    /// Omitted `permissions`: registry load succeeds normally, the gated
    /// discovery path spawns the server (unspecified == allow-all), and calls
    /// flow through registry, policy index, adapter, and pipeline untouched —
    /// identical to pre-§4.9 behavior. This is the compatibility guarantee.
    #[tokio::test]
    async fn compat_undeclared_manifest_keeps_legacy_behavior_end_to_end() {
        let probe = Probe::new();
        let toml_body = manifest_with("");
        let plugins_dir = probe.install_manifest(&toml_body);

        let mut plugin_registry = shannon_core::plugin::PluginRegistry::new(plugins_dir);
        plugin_registry.load_all().await.expect("load_all succeeds");
        let loaded = plugin_registry.get(PLUGIN_NAME).expect("plugin registered");
        assert!(loaded.enabled, "undeclared plugin still enabled by default");
        assert!(
            loaded.manifest.permissions.is_empty(),
            "compat fixture must be truly undeclared"
        );

        let policy = Arc::new(PluginPermissionPolicy::from_manifest(&loaded.manifest));
        let mut discovery = probe.discover_gated(&policy).await.expect("discovery runs");
        assert_eq!(discovery.tools.len(), 3);
        assert_eq!(spawn_count(&probe), 1, "one discovery-time spawn happened");

        let write_adapter = take_tool(&mut discovery, TOOL_WRITE);
        assert!(
            write_adapter.policy().is_some(),
            "policy rides along for per-call tracing"
        );

        let registry = Arc::new(ToolRegistry::new());
        registry.attach_plugin_policy(PLUGIN_NAME, Arc::clone(&policy));
        while let Some(tool) = discovery.tools.pop() {
            registry.register(Box::new(tool)).expect("registers");
        }
        registry
            .register(Box::new(write_adapter))
            .expect("registers");

        let service = service_for(&registry);
        let out = service
            .run_tool_use(Uuid::new_v4(), TOOL_WRITE, json!({}))
            .await
            .expect("undeclared manifest keeps every gate open");
        assert!(!out.output.is_error);
        assert!(
            probe.write_target.exists(),
            "write side effect happened exactly like before §4.9"
        );
        assert!(
            spawn_count(&probe) >= 2,
            "per-call cold spawn happened as usual"
        );
    }

    // ── execute_commands: spawn gate (negative) ────────────────────────────

    /// Declared only `read_files`: discovery refuses BEFORE spawning any
    /// process — no marker, no effects, unified denial naming the face.
    #[tokio::test]
    async fn discovery_refuses_spawn_when_execute_commands_undeclared() {
        let probe = Probe::new();
        let toml_body = manifest_with(r#"permissions = ["read_files"]"#);
        let _plugins_dir = probe.install_manifest(&toml_body);

        let manifest = probe.parse_manifest(&toml_body);
        assert_eq!(manifest.permissions.len(), 1);

        let policy = Arc::new(PluginPermissionPolicy::from_manifest(&manifest));
        let denied = match probe.discover_gated(&policy).await {
            Err(e) => e,
            Ok(_) => panic!("discovery must refuse without execute_commands"),
        };
        assert!(denied.starts_with(DENY_PREFIX), "{denied}");
        assert!(denied.contains("execute_commands"), "{denied}");
        assert!(denied.contains(PLUGIN_NAME), "{denied}");

        assert!(
            !probe.spawn_marker.exists(),
            "no subprocess may be spawned without the execute_commands grant"
        );
    }

    // ── mcp_tools: pipeline routing gate ───────────────────────────────────

    /// Declared only `execute_commands`: discovery succeeds and the server
    /// runs, but pipeline routing into its `mcp__…` namespace refuses before
    /// reaching the tool — markers stay frozen at the discovery baseline,
    /// proving interception happened prior to any per-call spawn. Tools in
    /// ungoverned namespaces are unaffected (scope isolation), and a sibling
    /// namespace carrying the mcp_tools grant passes (positive control).
    #[tokio::test]
    async fn pipeline_refuses_calls_without_mcp_tools_but_passes_those_with_it() {
        let probe = Probe::new();
        let toml_body = manifest_with(r#"permissions = ["execute_commands"]"#);
        let _plugins_dir = probe.install_manifest(&toml_body);

        let manifest = probe.parse_manifest(&toml_body);
        let strict_policy = Arc::new(PluginPermissionPolicy::from_manifest(&manifest));
        let mut discovery = probe
            .discover_gated(&strict_policy)
            .await
            .expect("execute_commands IS declared -> discovery proceeds");
        let baseline = spawn_count(&probe);
        assert!(baseline >= 1);

        let registry = Arc::new(ToolRegistry::new());
        // Owner policy lacks mcp_tools: the registry becomes the barrier.
        registry.attach_plugin_policy(PLUGIN_NAME, Arc::clone(&strict_policy));
        for tool in discovery.tools.drain(..) {
            registry.register(Box::new(tool)).expect("registers");
        }
        // An ungoverned MCP-shaped name from a different owner: untouched.
        registry
            .register(Box::new(EchoTool("mcp__other-owner__fancy".to_string())))
            .expect("registers");

        let service = service_for(&registry);

        let err = service
            .run_tool_use(Uuid::new_v4(), TOOL_WRITE, json!({}))
            .await
            .expect_err("routing refused when mcp_tools undeclared");
        match err {
            ToolExecutionError::ExecutionFailed(msg) => {
                // The service wraps channel wording in front; the unified
                // denial text must appear verbatim after it.
                assert!(msg.contains(DENY_PREFIX), "{msg}");
                assert!(msg.contains("mcp_tools"), "{msg}");
                assert!(msg.contains(PLUGIN_NAME), "{msg}");
            }
            other => panic!("expected routed execution failure, got {other:?}"),
        }
        assert_eq!(
            spawn_count(&probe),
            baseline,
            "refusal happens before any per-call spawn"
        );
        assert!(!probe.write_target.exists());

        // Scope isolation: other-owner tools run as usual.
        let out = service
            .run_tool_use(Uuid::new_v4(), "mcp__other-owner__fancy", json!({}))
            .await
            .expect("ungoverned namespace unaffected");
        assert!(!out.output.is_error);

        // Positive control: same name WITH the grant routes through the real
        // subprocess and completes the round trip.
        let granted_registry = Arc::new(ToolRegistry::new());
        let full_policy = Arc::new(PluginPermissionPolicy::from_permissions(
            FULL_PERMISSIONS_GRANTS.to_vec(),
        ));
        granted_registry.attach_plugin_policy(PLUGIN_NAME, Arc::clone(&full_policy));
        let mut allowed_discovery = probe
            .discover_gated(&full_policy)
            .await
            .expect("full grants pass discovery");
        let read_adapter = take_tool(&mut allowed_discovery, TOOL_READ);
        let output = read_adapter
            .execute(json!({}))
            .await
            .expect("call executes");
        assert!(!output.is_error);
        assert_eq!(output.content, "read:fixture-ok");
        assert!(owner_of_tool(TOOL_READ).expect("parses") == PLUGIN_NAME);
    }

    const FULL_PERMISSIONS_GRANTS: &[PluginPermission] = &[
        PluginPermission::ReadFiles,
        PluginPermission::WriteFiles,
        PluginPermission::ExecuteCommands,
        PluginPermission::Network,
        PluginPermission::McpTools,
    ];

    /// Fully declared tool plugin: nothing is over-blocked. Discovery, a
    /// write-effecting call and a command-effecting call all succeed with the
    /// physical effects present — declarations are honored 1:1.
    #[tokio::test]
    async fn declared_plugin_runs_every_face_end_to_end() {
        let probe = Probe::new();
        let toml_body = manifest_with(FULL_PERMISSIONS_LINE);
        let _plugins_dir = probe.install_manifest(&toml_body);

        let manifest = probe.parse_manifest(&toml_body);
        let policy = Arc::new(PluginPermissionPolicy::from_manifest(&manifest));
        let mut discovery = probe.discover_gated(&policy).await.expect("passes gates");

        let registry = Arc::new(ToolRegistry::new());
        registry.attach_plugin_policy(PLUGIN_NAME, Arc::clone(&policy));
        let service = service_for(&registry);

        for wanted in [TOOL_READ, TOOL_WRITE, TOOL_EXEC] {
            let tool = take_tool(&mut discovery, wanted);
            registry.register(Box::new(tool)).expect("registers");
        }

        let out = service
            .run_tool_use(Uuid::new_v4(), TOOL_WRITE, json!({}))
            .await
            .expect("write-face call allowed");
        assert!(!out.output.is_error);
        assert!(probe.write_target.exists());

        let out = service
            .run_tool_use(Uuid::new_v4(), TOOL_EXEC, json!({}))
            .await
            .expect("exec-face call allowed");
        assert!(!out.output.is_error);
        assert!(probe.exec_target.exists());

        assert!(spawn_count(&probe) >= 1);
    }

    // ── network: remote transport gate at HTTP discovery ────────────────────

    /// `gated_discover_tools_http` refuses before opening any socket when the
    /// network face is not granted; with the grant it makes a genuine attempt
    /// (which fails for ordinary connectivity reasons, never as a denial).
    #[tokio::test]
    async fn http_discovery_gates_on_network_declaration() {
        let probe = Probe::new();

        let toml_body = manifest_with(r#"permissions = ["write_files"]"#);
        let _plugins_dir = probe.install_manifest(&toml_body);
        let manifest = probe.parse_manifest(&toml_body);
        let offline_policy = Arc::new(PluginPermissionPolicy::from_manifest(&manifest));

        let denied = match gated_discover_tools_http(
            &offline_policy,
            PLUGIN_NAME,
            "http://127.0.0.1:9/sse",
            HashMap::new(),
        )
        .await
        {
            Err(e) => e,
            Ok(_) => panic!("http discovery must refuse without the network grant"),
        };
        assert!(denied.starts_with(DENY_PREFIX), "{denied}");
        assert!(denied.contains("network"), "{denied}");
        assert!(denied.contains(PLUGIN_NAME), "{denied}");

        // Grant network: the gate opens and the failure (dead socket) carries
        // ordinary transport wording, not a denial prefix.
        let online_policy = Arc::new(PluginPermissionPolicy::from_permissions(vec![
            PluginPermission::Network,
            PluginPermission::McpTools,
        ]));
        if let Err(e) = gated_discover_tools_http(
            &online_policy,
            PLUGIN_NAME,
            "http://127.0.0.1:9/sse",
            HashMap::new(),
        )
        .await
        {
            assert!(
                !e.starts_with(DENY_PREFIX),
                "declared network must not be denied: {e}"
            );
        }
    }

    // ── registration-sequence mirror for prompt-based extensions ────────────

    /// Mirrors the repl/CLI registration sequence for prompt-based
    /// extensions (`admit_prompt_based_extension` then the entry-file read):
    /// both faces granted -> the host really reads the template; `llm_api`
    /// missing -> refusal precedes any read and names the field, so no
    /// extension content enters the conversation without both grants.
    #[test]
    fn skill_extension_registration_respects_prompt_faces() {
        let dir = TempDir::new().expect("temp plugin dir");
        let entry_path = dir.path().join("TEMPLATE.md");
        fs::write(&entry_path, "hello-from-template").expect("entry written");

        // Both faces declared: admission passes and the entry is read.
        let full = PluginPermissionPolicy::from_permissions(vec![
            PluginPermission::LlmApi,
            PluginPermission::ReadFiles,
        ]);
        admit_prompt_based_extension(&full, "skill-plugin").expect("both faces granted");
        let template = fs::read_to_string(&entry_path).expect("host reads entry file");
        assert_eq!(template, "hello-from-template");

        // llm_api undeclared: registration refused before any read occurs;
        // the unified denial names the plugin and the missing field.
        let no_llm = PluginPermissionPolicy::from_permissions(vec![PluginPermission::ReadFiles]);
        let err = admit_prompt_based_extension(&no_llm, "skill-plugin")
            .expect_err("llm_api undeclared -> refused");
        let text = err.to_string();
        assert!(text.starts_with(DENY_PREFIX), "{text}");
        assert!(text.contains("llm_api"), "{text}");
        assert!(text.contains("'skill-plugin'"), "{text}");
    }
}
