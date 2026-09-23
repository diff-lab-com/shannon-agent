//! Single-load plugin bootstrap for the REPL.
//!
//! Review §P2-22: `Repl::new` used to load `~/.shannon/plugins/` twice —
//! once for tool registration and once more inside the command-registry
//! build — so every stdio plugin process was spawned twice per startup.
//! The load now happens exactly once and the two registration halves
//! consume the same loaded snapshot:
//!
//! - [`register_plugin_tools`] — tool plugins: exactly one stdio
//!   discovery spawn per plugin, tools registered into the shared
//!   `ToolRegistry`;
//! - the command-registry builder in `Repl::new` — Command / Skill
//!   plugins become prompt commands (no process spawn at all).

use shannon_core::plugin::InstalledPlugin;
use shannon_core::tools::ToolRegistry;

/// Register tool plugins from a loaded [`shannon_core::plugin::PluginRegistry`]
/// snapshot.
///
/// Command / Skill plugins are deliberately NOT handled here — they carry
/// no stdio transport and must never spawn a process; `Repl::new` turns
/// them into prompt commands when it builds the command registry.
pub(crate) async fn register_plugin_tools(
    tool_registry: &ToolRegistry,
    plugins: &[&InstalledPlugin],
) {
    for plugin in plugins {
        // §4.9: gate every Shannon-side execution point on the manifest
        // allow-set; empty declarations keep the pre-enforcement lenient
        // default.
        let policy = std::sync::Arc::new(
            shannon_core::plugin::PluginPermissionPolicy::from_manifest(&plugin.manifest),
        );
        // write_files enforcement ("declaration IS sandbox"): a declared
        // write_files face installs a manifest-derived execution world
        // around every stdio spawn (discovery + per-call); anything else
        // stays a zero-overhead passthrough.
        let spawn_guard = shannon_tools::sandbox::plugin_spawn_guard_for_manifest(
            &policy,
            &plugin.manifest.name,
            &plugin.path,
        );
        match plugin.manifest.kind() {
            Ok(shannon_core::plugin::PluginKind::Tool { transport }) => {
                if let Some(command) = transport.command() {
                    let args = transport.args().to_vec();
                    match shannon_core::plugin::gated_discover_tools_stdio_guarded(
                        &policy,
                        &plugin.manifest.name,
                        command,
                        &args,
                        &std::collections::HashMap::new(),
                        None,
                        spawn_guard,
                    )
                    .await
                    {
                        Ok(result) => {
                            tool_registry.attach_plugin_policy(
                                &plugin.manifest.name,
                                std::sync::Arc::clone(&policy),
                            );
                            let tool_count = result.tools.len();
                            for tool in result.tools {
                                if let Err(e) = tool_registry.register(Box::new(tool)) {
                                    tracing::debug!("Plugin tool registration skipped: {}", e);
                                }
                            }
                            tracing::info!(
                                "Registered {} tool(s) from plugin '{}'",
                                tool_count,
                                plugin.manifest.name
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Plugin '{}' tool discovery failed: {e}",
                                plugin.manifest.name
                            );
                        }
                    }
                }
            }
            // Command / Skill plugins are registered as prompt commands by
            // the command-registry builder in `Repl::new` (single §P2-22
            // load) — they spawn no process here.
            Ok(shannon_core::plugin::PluginKind::Command { .. })
            | Ok(shannon_core::plugin::PluginKind::Skill { .. }) => {}
            Err(e) => {
                tracing::warn!("Plugin '{}' has invalid config: {e}", plugin.manifest.name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal MCP stdio server: records every spawn into a marker file,
    /// then answers `initialize` / `tools/list` / `prompts/list` like a
    /// real tool plugin would.
    const FAKE_SERVER_SH: &str = r#"#!/bin/sh
echo spawned >> "__SPAWN_MARKER__"
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"probe","version":"1.0.0"}}}'
      ;;
    *'"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"probe_tool"}]}}'
      ;;
    *'"prompts/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"prompts":[]}}'
      ;;
  esac
done
"#;

    const TOOL_MANIFEST: &str = r#"
name = "probe-tool"
version = "1.0.0"
description = "spawn-count probe"
type = "tool"
entry = "server.sh"
permissions = ["execute_commands", "mcp_tools"]

[transport]
type = "stdio"
command = "sh"
args = ["__SCRIPT__"]
"#;

    const SKILL_MANIFEST: &str = r#"
name = "probe-skill"
version = "1.0.0"
description = "no-transport probe"
type = "skill"
entry = "template.md"
trigger = "/probe"
template = "hi"
"#;

    fn spawn_count(marker: &std::path::Path) -> usize {
        std::fs::read_to_string(marker)
            .unwrap_or_default()
            .lines()
            .count()
    }

    /// §P2-22 regression: one tool-discovery pass spawns each stdio plugin
    /// process exactly once (the old `Repl::new` ran the load twice), and
    /// non-tool plugins never spawn anything.
    #[tokio::test]
    async fn tool_plugin_spawns_once_and_non_tool_plugin_never_spawns() {
        let plugins_dir = tempfile::TempDir::new().expect("tempdir");
        let marker = plugins_dir.path().join("spawn_markers.txt");
        let script = plugins_dir.path().join("probe_server.sh");
        std::fs::write(
            &script,
            FAKE_SERVER_SH.replace("__SPAWN_MARKER__", &marker.to_string_lossy()),
        )
        .expect("write fake server script");

        let tool_dir = plugins_dir.path().join("probe-tool");
        std::fs::create_dir_all(&tool_dir).expect("tool plugin dir");
        std::fs::write(
            tool_dir.join("plugin.toml"),
            TOOL_MANIFEST.replace("__SCRIPT__", script.to_str().expect("utf-8 path")),
        )
        .expect("write tool manifest");

        let skill_dir = plugins_dir.path().join("probe-skill");
        std::fs::create_dir_all(&skill_dir).expect("skill plugin dir");
        std::fs::write(skill_dir.join("plugin.toml"), SKILL_MANIFEST)
            .expect("write skill manifest");

        let mut registry =
            shannon_core::plugin::PluginRegistry::new(plugins_dir.path().to_path_buf());
        registry.load_all().await.expect("both fixtures load");
        let enabled = registry.list_enabled();
        assert_eq!(enabled.len(), 2, "tool + skill fixtures must both load");

        let tool_registry = ToolRegistry::new();
        register_plugin_tools(&tool_registry, &enabled).await;

        assert_eq!(
            spawn_count(&marker),
            1,
            "stdio plugin process must be spawned exactly once"
        );
        assert!(
            tool_registry
                .list()
                .iter()
                .any(|name| name.starts_with("mcp__probe-tool__")),
            "tool plugin's tools must be registered"
        );
    }
}
