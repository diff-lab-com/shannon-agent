# Plugin manifest permission semantics (`permissions` = allow-set)

This document pins the enforcement semantics of the `permissions` list in
`plugin.toml` / `.claude-plugin/plugin.json` (master plan §4.9, gap P7;
probe evidence in `crates/shannon-core/tests/plugin_permission_enforcement.rs`,
originally established by §4.5 in commit `5f83afc6`).

## Core principle

**A declaration IS the allow-set, and its effect domain is Shannon-side.**

- A permission entry does not *describe* what a plugin may do inside its own
  process — nothing outside the process can observe that. It *grants* the host
  the right to exercise one concrete capability face **on the plugin's behalf**
  at a named execution point.
- Declared faces are allowed at their points; anything else declared-set-wise
  is refused with a unified denial:

  ```
  plugin permission denied: plugin '<name>' is not granted '<permission>'
    — manifest declares [<set>] (declaration = allow-set; add '<permission>'
    to the plugin.toml permissions)
  ```

  The single error type is `PluginPermissionError`
  (`crates/shannon-core/src/plugin/permissions.rs`); every channel embeds its
  text verbatim. Every gate also emits a `target = "permission/decision"`
  tracing event (`decision = "allow" | "deny"`) that the §4.8 event bus will
  subscribe and persist into the L0 session log as
  `SessionEventKind::PermissionDecision`. No new `QueryEvent` variants were
  introduced for this.

## Default: undeclared = unchanged (allow-all)

An omitted or empty `permissions` list deserializes to an empty vec and is
treated as "nothing declared": **every gate stays open**, byte-for-byte
preserving pre-§4.9 behavior. This is deliberate compatibility (evaluation doc
risk register: "权限声明缺省 = 现状（宽松），显式声明才收紧") — explicit
declarations tighten; absence never does. Existing plugins without a
`permissions` key behave exactly as before, including all tests.

## Execution-point matrix (post-§4.9)

| Permission      | Shannon-side execution point                                                       | Gate location |
|-----------------|--------------------------------------------------------------------------------------|---------------|
| `execute_commands` | Spawning the plugin's stdio server binary — at discovery time **and** at every per-call cold spawn (`McpToolAdapter::execute`) | `gated_discover_tools_stdio`, adapter spawn gate |
| `network`       | Opening the plugin's remote HTTP/SSE transport — HTTP discovery and every per-call POST (`execute_remote`) | `gated_discover_tools_http`, adapter transport gate |
| `mcp_tools`     | Routing model/tool-pipeline calls to `mcp__<plugin>__*` registry entries             | `ToolRegistry::attach_plugin_policy` + `check_plugin_permission` in `execute`/`execute_streaming` (covers query engine, streaming scheduler, and `ToolExecutionService` facade) |
| `read_files`    | Host reading a command/skill extension's entry/template file during registration     | `admit_entry_read` via `admit_prompt_based_extension` |
| `llm_api`       | Prompt-based extensions (command/skill) driving model turns                          | `admit_prompt_based_extension`; refusal skips registration of `/plugin:<name>` commands and skill triggers |
| `write_files`   | **Reserved** — no post-manifest host-side write face exists today (installation necessarily writes before the manifest exists). Scaffolding ships as `PluginPermissionPolicy::allows_file_writes`; real enforcement arrives with the §4.11 FileSystemProvider seam, per the staged roadmap (权限强制 → 接缝化) | scaffolding only |

### Why these boundaries are honest

`read_files`, `write_files`, and parts of `execute_commands`/`network` also
have effects *inside* the plugin subprocess which no external observer can
intercept — this was the §4.5 probe verdict ("declared-but-unenforced"). The
fix therefore scopes each field to where coercion is actually possible for the
host and documents the rest, preferring narrow-and-true over wide-and-fake.
In particular: refusing a stdio spawn blocks every effect a not-yet-running
server could cause; interior side effects of an already-permitted server
remain outside any host enforcement domain by construction.

## Recipe: what to declare

- Tool plugin, stdio transport: `["execute_commands", "mcp_tools"]` plus faces
  your tool effects claim (`"read_files"`, `"write_files"`, `"network"`).
  `execute_commands` is required because running your binary IS command
  execution performed by Shannon on your behalf; `mcp_tools` is required
  because your tools participate in the host MCP namespace.
- Tool plugin, remote (sse/http): `["network", "mcp_tools"]`.
- Command plugin: `["llm_api", "read_files"]`.
- Skill plugin: `["llm_api", "read_files"]`.

## Migration hint

Adding a `permissions` key changes behavior only for the declaring plugin:
list everything the plugin needs from the host, or leave the key absent to
keep the legacy allow-all default. A declaration missing a face produces a
denial naming exactly the field to add.
