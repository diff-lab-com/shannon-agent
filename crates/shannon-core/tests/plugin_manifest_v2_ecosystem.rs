//! §4.10 W3-2 — three-kind sample-plugin end-to-end suite.
//!
//! Walks the real installer → registry → usage pipeline with generated
//! fixtures (no repo-root debris): a **skill** authored in v2 TOML, a
//! **command** authored in the Claude JSON dialect, and a **tool** (stdio
//! MCP server) authored in legacy v1 TOML. Each leg proves "install →
//! load → use" plus the §4.9 gates still firing on top of v2 plumbing, and
//! that an over-reaching declaration is refused ("越权拒绝").
//!
//! Clone-based installs are exercised through the same `install_from_path`
//! entry point the git flow funnels into after cloning (`copy_dir_contents`
//! + identical validation), so local-path fixtures are behaviorally
//! equivalent to `clone → install`.

use shannon_core::plugin::{
    DENY_PREFIX, PluginKind, PluginPermission, PluginPermissionPolicy, admit_prompt_based_extension,
};
use shannon_tool_interface::Tool as _;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Write plugin files under `<root>/<name>/` and return the source dir.
fn write_plugin(root: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).expect("fixture plugin dir");
    for (path, body) in files {
        let target = dir.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("fixture parent");
        }
        fs::write(target, body).expect("fixture file");
    }
    dir
}

// ── Leg 1 · skill, v2 TOML ────────────────────────────────────────────────

const V2_SKILL_TOML: &str = r#"
manifest_version = "2"
name = "craft-commit"
version = "1.0.0"
description = "opinionated commit drafting"
type = "skill"
entry = "template.md"
trigger = "/craft"
template = "Draft a commit for: {{input}}"

permissions = ["read_files", "llm_api"]

[[hooks]]
event = "UserPromptSubmit"
handler = "hooks/prep.sh"

[compat]
min = "0.1.0"
"#;

#[tokio::test]
async fn v2_skill_installs_loads_and_admits_prompt_extension() {
    let tmp = TempDir::new().expect("tmp");
    let src = write_plugin(
        tmp.path(),
        "src-craft",
        &[
            ("plugin.toml", V2_SKILL_TOML),
            ("template.md", "# craft\n{{input}}"),
            ("hooks/prep.sh", "echo prep"),
        ],
    );

    // install via the shared installer entry point
    let plugins_root = tmp.path().join("plugins");
    let mut registry = shannon_core::plugin::PluginRegistry::new(plugins_root.clone());
    let name = registry
        .install_from_path(&src)
        .await
        .expect("complete v2 skill installs");

    assert_eq!(name, "craft-commit");
    let installed = registry.get("craft-commit").expect("registered");
    assert_eq!(
        installed.manifest.schema_version(),
        shannon_core::plugin::ManifestVersion::V2
    );

    // reload from disk like the REPL/CLI do at startup
    drop(registry);
    let mut reloaded = shannon_core::plugin::PluginRegistry::new(plugins_root);
    reloaded.load_all().await.expect("reload is clean");
    assert!(reloaded.contains("craft-commit"));

    // use it: admission gates (read_files + llm_api) and entry read
    let plugin = reloaded.get("craft-commit").unwrap();
    let policy = PluginPermissionPolicy::from_manifest(&plugin.manifest);
    admit_prompt_based_extension(&policy, "craft-commit").expect("declared faces admit");
    let entry_bytes = fs::read(plugin.path.join(&plugin.manifest.entry)).expect("entry read");
    assert!(String::from_utf8_lossy(&entry_bytes).contains("{{input}}"));

    match plugin.manifest.kind().expect("kind resolves") {
        PluginKind::Skill { trigger, .. } => assert_eq!(trigger, "/craft"),
        other => panic!("expected Skill kind, got {other:?}"),
    }
}

// ── Leg 2 · command, Claude JSON dialect (+ overreach refusal) ────────────

const CLAUDE_COMMAND_JSON: &str = r#"{
  "name": "triage-kit",
  "version": "0.2.0",
  "description": "Issue triage slash-command",
  "type": "command",
  "entry": "triage.md",
  "command_name": "triage",
  "command_description": "Draft a triage plan",
  "permissions": ["read_files", "llm_api"],
  "mcpServers": { "triage-sidecar": { "command": "npx", "args": ["-y", "noop"] } },
  "keywords": ["triage"]
}"#;

#[tokio::test]
async fn claude_command_installs_and_overreach_is_refused() {
    let tmp = TempDir::new().expect("tmp");
    let src = write_plugin(
        tmp.path(),
        "src-triage",
        &[
            (".claude-plugin/plugin.json", CLAUDE_COMMAND_JSON),
            ("triage.md", "Triage plan: …"),
        ],
    );

    let mut registry = shannon_core::plugin::PluginRegistry::new(tmp.path().join("plugins"));
    registry
        .install_from_path(&src)
        .await
        .expect("claude dialect installs (legacy-lenient completeness)");

    let plugin = registry
        .get("triage-kit")
        .expect("registered from claude json");
    match plugin.manifest.kind().unwrap() {
        PluginKind::Command { name, description } => {
            assert_eq!(name, "triage");
            assert_eq!(description, "Draft a triage plan");
        }
        other => panic!("expected Command kind, got {other:?}"),
    }
    // The bundled mcpServers map parsed into references.
    assert_eq!(plugin.manifest.mcp.len(), 1);
    assert_eq!(plugin.manifest.mcp[0].name, "triage-sidecar");

    // declared faces admit the prompt-driven extension
    let policy = PluginPermissionPolicy::from_manifest(&plugin.manifest);
    admit_prompt_based_extension(&policy, "triage-kit").expect("admitted");

    // 越权拒绝: strip llm_api from a copy of the policy — registration must refuse.
    let stripped = PluginPermissionPolicy::from_permissions(vec![PluginPermission::ReadFiles]);
    let err = admit_prompt_based_extension(&stripped, "triage-kit").unwrap_err();
    assert_eq!(err.required, PluginPermission::LlmApi);
    assert!(
        err.to_string().starts_with(DENY_PREFIX),
        "denial renders unified prefix: {err}"
    );
}

// ── Leg 3 · tool, legacy v1 TOML over a real stdio MCP server ─────────────

const V1_TOOL_TOML_TEMPLATE: &str = r#"
name = "svg-lint"
version = "1.0.0"
description = "legacy v1 tool wrapping an MCP stdio server"
type = "tool"
entry = "server.sh"
__PERMISSIONS__

[transport]
type = "stdio"
command = "sh"
args = [__SCRIPT__]
"#;

const FAKE_SERVER_SH: &str = r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"svg-lint","version":"1.0.0"}}}'
      ;;
    *'"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"lint"},{"name":"stats"}]}}'
      ;;
    *'"prompts/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"prompts":[]}}'
      ;;
  esac
done
"#;

fn manifest_toml(perms_line: &str, script: &Path) -> String {
    V1_TOOL_TOML_TEMPLATE
        .replace("__PERMISSIONS__\n", perms_line)
        .replace(
            "__SCRIPT__",
            &format!("\"{}\"", script.to_str().expect("utf8 path")),
        )
}

#[cfg(unix)]
#[tokio::test]
async fn v1_tool_installs_discovers_over_gated_stdio_and_refuses_undeclared_spawn() {
    use shannon_core::plugin::{PluginRegistry, gated_discover_tools_stdio as discover};
    use std::collections::HashMap;
    use std::sync::Arc;

    let tmp = TempDir::new().expect("tmp");
    let script = tmp.path().join("server.sh");
    fs::write(&script, FAKE_SERVER_SH).expect("write fake server");

    // Legacy shape: omitted permissions entirely (the pre-enforcement default).
    let src = write_plugin(
        tmp.path(),
        "src-svg-lint",
        &[
            ("plugin.toml", &manifest_toml("", &script)),
            ("server.sh", FAKE_SERVER_SH),
        ],
    );

    let mut registry = PluginRegistry::new(tmp.path().join("plugins"));
    registry
        .install_from_path(&src)
        .await
        .expect("v1 lenient install succeeds");

    let plugin = registry.get("svg-lint").expect("tool registered").clone();
    match &plugin.manifest.kind().expect("kind") {
        PluginKind::Tool { transport } => {
            assert!(transport.is_stdio());
            assert_eq!(transport.command(), Some("sh"));
        }
        other => panic!("expected Tool kind, got {other:?}"),
    }

    // undeclared ⇒ allow-all (documented v1 compatibility default)
    let lenient = Arc::new(PluginPermissionPolicy::unspecified());
    let discovered = discover(
        &lenient,
        "svg-lint",
        "sh",
        &[script.to_string_lossy().into_owned()],
        &HashMap::new(),
        None,
    )
    .await
    .expect("undeclared v1 keeps spawning");
    // adapters expose the routed names mcp__<plugin>__<tool>
    let names: Vec<String> = discovered
        .tools
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    assert!(
        names.iter().any(|n| n.ends_with("__lint")) && names.iter().any(|n| n.ends_with("__stats")),
        "{names:?}"
    );

    // 越权拒绝: same shape but *declaring* without execute_commands ⇒ spawn refused.
    let tight = Arc::new(PluginPermissionPolicy::from_permissions(vec![
        PluginPermission::McpTools,
        PluginPermission::Network,
    ]));
    let err = discover(
        &tight,
        "svg-lint",
        "sh",
        &[script.to_string_lossy().into_owned()],
        &HashMap::new(),
        None,
    )
    .await
    .err()
    .expect("undeclared execute_commands must refuse spawn");
    assert!(
        err.contains(DENY_PREFIX) && err.contains("execute_commands"),
        "{err}"
    );
}

// ── Broken manifests surface instead of vanishing (integration view) ─────

#[tokio::test]
async fn corrupted_sibling_manifests_are_reported_while_valid_plugins_still_work() {
    let tmp = TempDir::new().expect("tmp");

    write_plugin(
        tmp.path(),
        "good-skill",
        &[("plugin.toml", V2_SKILL_TOML), ("template.md", "hi")],
    );
    write_plugin(
        tmp.path(),
        "corrupt-a",
        &[("plugin.toml", "name = \"no closing quote")],
    );
    write_plugin(
        tmp.path(),
        "corrupt-b",
        &[(
            "plugin.toml",
            "version = \"1.0.0\"\ndescription = \"missing name\"\n",
        )],
    );

    let mut registry = shannon_core::plugin::PluginRegistry::new(tmp.path().to_path_buf());
    let err = registry
        .load_all()
        .await
        .expect_err("aggregated failure for corrupt siblings");
    let report = err.to_string();
    assert!(
        report.contains("corrupt-a") && report.contains("corrupt-b"),
        "{report}"
    );

    // …and the healthy plugin remains usable through the normal listing path.
    assert_eq!(registry.list_enabled().len(), 1);
    assert!(registry.contains("craft-commit"));
}
