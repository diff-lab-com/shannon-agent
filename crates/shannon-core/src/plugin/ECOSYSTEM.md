# Shannon Plugin Ecosystem Conventions (§4.10 W3-2)

Author-facing contract for packaging, naming, distributing, and validating
Shannon extensions. Companion to [`PERMISSIONS.md`](./PERMISSIONS.md), which
defines what the `permissions` declaration means at each Shannon-side
execution point; this document defines the *format* and the *checks*.

---

## 1. Naming and discovery

- Tag publishable plugin repositories with the GitHub topic **`shannon-plugin`**
  so they are discoverable via topic search and candidate for the community
  index (`registry_url` in `[plugins]` config).
- Repository name SHOULD equal the manifest `name` (lowercase, `[a-z0-9_-]`);
  `shannon /plugin install <git-url-or-path>` derives the install directory
  from the manifest name either way.

## 2. Manifest formats — the reading matrix

Shannon reads three dialects. Preference order inside one plugin directory:
**`plugin.toml` → `.claude-plugin/plugin.json`** (first present wins; archives
additionally accept a root `manifest.json`).

| capability | v1 TOML (`plugin.toml`) | v2 TOML (`manifest_version = "2"`) | Claude JSON (`.claude-plugin/plugin.json`) |
|---|---|---|---|
| base fields (`name`, `version`, `description`, `type`, `entry`) | required | required | required |
| permissions | optional list | **list expected to be complete** (see §4) | optional list (read as legacy) |
| MCP references | — | `[[mcp]]` rows or nothing | top-level `mcpServers` object map |
| hook subscriptions | — | `[[hooks]]` rows (reserved slot, validated) | — (not parsed from Claude hooks files) |
| compat window | `min_shannon_version` (soft) | `[compat] min / max` (warn-only outside window) | — |
| reserved types | — | `type = "wasm"` accepted by schema, refused at load ("reserved §4.16 slot") | — |

Both TOML and JSON parse into one in-memory shape, so all downstream
machinery (kinds, transports, permission policy) is dialect-blind.

### v2 template: skill

```toml
manifest_version = "2"
name = "commit-craft"
version = "1.0.0"
description = "Opinionated commit-message drafting skill"
license = "MIT"
keywords = ["git", "commits"]
type = "skill"
entry = "template.md"
trigger = "/craft-commit"

permissions = ["read_files", "llm_api"]

[[hooks]]
event = "UserPromptSubmit"   # HookEventType spelling; validated at install
handler = "hooks/prep.sh"    # relative path; reserved (not yet executed)

[compat]
min = "0.10.0"
max = "0.12.0"               # exclusive upper bound
```

### v2 template: command

```toml
manifest_version = "2"
name = "triage-kit"
version = "0.2.0"
description = "Issue triage slash-command suite"
type = "command"
entry = "triage.md"
command_name = "triage"
command_description = "Draft a triage plan for the current repo"

permissions = ["read_files", "llm_api"]
```

Claude-dialect equivalent:

```json
{
  "name": "triage-kit",
  "version": "0.2.0",
  "description": "Issue triage slash-command suite",
  "type": "command",
  "entry": "triage.md",
  "command_name": "triage",
  "command_description": "Draft a triage plan for the current repo",
  "permissions": ["read_files", "llm_api"]
}
```

### v2 template: tool (MCP server wrapper)

```toml
manifest_version = "2"
name = "svg-lint"
version = "1.1.0"
description = "Lint SVG assets through a bundled MCP server"
type = "tool"
entry = "server/index.js"

[transport]
type = "stdio"
command = "node"
args = ["server/index.js"]

permissions = ["execute_commands", "mcp_tools", "network"]

# Optional bundled/proxied servers — each becomes mcp__<name>__* tools.
[[mcp]]
name = "sidecar-render"
command = "npx"              # type = "stdio" is the default when omitted
args = ["-y", "svg-render", "--serve"]

[[mcp]]
name = "remote-gallery"
type = "sse"
url = "https://gallery.example.test/sse"
```

## 3. Install-time validation rules

Run identically for `/plugin install <path|git>`, `.dxt`/`.mcpb` archives,
and plugin `update`. Failure **refuses installation before any file lands**
in the plugins directory.

Schema checks (all dialects):

1. `name`, `description` non-empty; `version` parses as `MAJOR.MINOR.PATCH`
   (optional leading `v`, pre-release/build suffix ignored).
2. `entry` is non-empty and stays *inside* the plugin directory (no absolute
   paths, no `..` components).
3. The declared kind resolves (`kind()`): tool ⇒ `[transport]` present with
   `command` (stdio) or `url` (sse); command ⇒ `command_name`; skill ⇒
   `trigger` + `template`. Unknown types are rejected; `wasm` reports the
   reserved-slot message specifically.
4. Every `[[hooks]]` row names a real `HookEventType` (PascalCase) and its
   handler is a safe relative path.
5. Every `[[mcp]]` reference has a unique name within the manifest and a
   complete transport (`stdio` ⇒ `command`, `sse`/`http` ⇒ `url`).

Permission-completeness check (the shape-implied faces from §4 below):

- **v2**: missing implied faces are a hard error, e.g.
  `v2 permission completeness: plugin 'svg-lint' declares [execute_commands] but its shape implies execute_commands, network, mcp_tools — declare network, mcp_tools or drop manifest_version = "2"`.
- **v1 / claude**: gaps become warnings logged during install/load, e.g.
  `plugin 'legacy' declares permissions [] but omits implied face(s) read_files, llm_api — runtime gates will refuse those actions until they are added (declaration = allow-set)`.

Compatibility window: if `[compat]` excludes the running Shannon, install
still succeeds but logs
`plugin '<n>' declares a compat window outside this Shannon (<ver>)…`.

## 4. Implied permission faces

| manifest shape | implies | because (see PERMISSIONS.md) |
|---|---|---|
| `type = "tool"` + stdio transport | `execute_commands` | host spawns the server process |
| `type = "tool"` + sse/http transport | `network` | host opens the remote transport |
| any `type = "tool"` (or any `[[mcp]]`) | `mcp_tools` | calls route as `mcp__<plugin>__*` through the registry gate |
| bundling an stdio `[[mcp]]` | `execute_commands` | same spawn gate, plugin's own policy |
| bundling a remote `[[mcp]]` | `network` | same transport gate |
| `type = "command"` / `"skill"` | `read_files`, `llm_api` | host reads the entry/template; the prompt drives model turns (`admit_prompt_based_extension`) |

Empty declarations: v1 semantics keep omitted/empty `permissions` as the
lenient allow-all default (documented compatibility load-bearing choice);
v2's completeness rule effectively requires declaring every implied face.

## 5. Distribution entry points

- `shannon /plugin install https://github.com/<org>/<repo>` — git clone of a
  topic-tagged repository carrying any of the three formats;
- local paths (development loop): `/plugin install ./my-plugin`;
- Desktop Extensions: `.dxt` / `.mcpb` archives with `plugin.toml`,
  `.claude-plugin/plugin.json`, or root `manifest.json` inside.

## 6. Provenance debugging

`shannon --dump-config` prints effective configuration as provenance-annotated
JSON — layer ladder low→high (builtin → user-global → project → env-vars →
connected → cli-overlay), each entry labeled with the higher layer that
overrides it. Note on rendering: a key whose value equals the engine default
(e.g. `debug = false` never written anywhere) renders as *unset*, i.e. layers
show intent rather than byte-exact echoes.

## 7. Versioning the format itself

Adding fields is additive; unknown keys in manifests are ignored today.
When validation must distinguish generations, gate on
`manifest_version`: absent/`"1"` keeps 2024-era behavior forever, `"2"` opts
into tightened checks. Bump to `"3"` only for a semantic break that cannot
be expressed additively.
