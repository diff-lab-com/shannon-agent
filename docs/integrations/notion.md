# Notion MCP server (P1-3 v4)

Shannon ships a dedicated Notion MCP server that exposes six tools over
stdio JSON-RPC 2.0:

| Tool                    | Scope | Notes                                                              |
|-------------------------|-------|--------------------------------------------------------------------|
| `notion_search_pages`   | read  | Workspace search; returns `{ results, has_more, next_cursor }`    |
| `notion_get_page`       | read  | Single page by id; returns the page object with `properties`      |
| `notion_append_block`   | write | Append a block to a page's children                                 |
| `notion_create_page`    | write | Create a page in a database or under another page                  |
| `notion_list_databases` | read  | List databases visible to the integration                          |
| `notion_query_database` | read  | Query a database with optional filter / sorts / pagination         |

The server targets `https://api.notion.com/v1/` and pins the wire schema
to Notion REST version `2022-06-28` via the `Notion-Version` header.
Unlike Slack and Jira, Notion's API uses **internal-integration tokens**
(no OAuth flow) — the user pastes a secret into Shannon and the server
sends it as `Authorization: Bearer <secret>` on every call.

---

## 1. Build & launch

The Notion module lives behind the `notion` Cargo feature. It is
**not** in the default feature set (Slack + Jira are) so a contributor
who only wants to ship Slack/Jira can opt out without dragging in the
Notion dependency surface:

```toml
# crates/shannon-mcp-saas/Cargo.toml
[features]
default = ["slack", "jira"]
slack   = []
jira    = []
notion  = []
```

Build with Notion enabled and launch the backend:

```bash
cargo build -p shannon-mcp-saas --no-default-features --features "slack,jira,notion" --release
./target/release/shannon-mcp-saas notion
# prints the registered tool listing to **stderr**, then enters the
# stdio JSON-RPC loop on stdin/stdout.
```

The host (Shannon CLI / desktop) spawns the process and pipes JSON-RPC
frames through its stdin. On `initialize` the server returns the six
Notion tools plus the standard MCP `serverInfo`.

---

## 2. Authentication

Notion's REST API is authenticated by an **internal-integration
secret** — a `secret_…` (legacy) or `ntn_…` (current) opaque string
that the user creates at `Settings → Integrations → Develop your own
integrations` in the Notion UI. The integration must be **explicitly
shared with each page or database** it should access; Notion will
return `404` on any object that has not been shared, which is by
design.

### 2.1 Create an internal integration

1. Visit <https://www.notion.so/profile/integrations>.
2. Click **Develop your own integrations** → **New integration**.
3. Choose a name (e.g. "Shannon") and a workspace; **Internal**
   integration type is sufficient for the six tools this server
   exposes. Public integrations require OAuth and are out of scope for
   P1-3 v4.
4. Copy the **Internal Integration Secret** (`secret_…` or `ntn_…`).
   Treat it like a password — anyone with the secret can read every
   page that has been shared with the integration.

### 2.2 Share pages with the integration

Notion's permission model is **per-page**, not per-workspace. For each
top-level page or database the agent should access:

1. Open the page in Notion.
2. Click `…` → **Connections** → search for the integration name
   (e.g. "Shannon") → **Confirm**.

Until you share at least one page, every API call returns
`{"object":"error","status":404,"code":"object_not_found",…}` even on
resources that exist. This is the most common first-run gotcha; see
§7.2 for the matching troubleshooting entry.

### 2.3 Pick a delivery channel

Two paths, same `TokenProvider` interface:

| Method            | When to use                                | Storage                                  |
|-------------------|--------------------------------------------|------------------------------------------|
| **Env var**       | Headless / CI / scripted agent runs        | `NOTION_TOKEN` env var                   |
| **Keyring**       | Interactive desktop / CLI sessions        | keyring `shannon-mcp-saas/notion-token`  |

`TokenProvider::get_token()` resolves them in that priority order:
`NOTION_TOKEN` env → keyring.

#### 2.3.1 Env var (headless)

```bash
export NOTION_TOKEN="ntn_…"
shannon-mcp-saas notion
```

#### 2.3.2 Keyring (interactive)

The shannon desktop app writes the secret to the keyring on
`connections connect notion`. The entry layout is:

| Service            | Account         | Contents                  |
|--------------------|-----------------|---------------------------|
| `shannon-mcp-saas` | `notion-token`  | Internal-integration secret |

The service is `shannon-mcp-saas` (the crate name) rather than
`shannon` (Slack/Jira) so a rebrand or a future split between the
desktop binary and the server-side SaaS runner can re-target without
colliding.

`TokenProvider::save_to_keyring` stores the value verbatim — Notion
tokens are already opaque and self-authenticating. To rotate, run
`connections connect notion` again; `TokenProvider::clear` removes the
entry (idempotent — succeeds even if no entry exists).

---

## 3. Configuration

`~/.shannon/mcp-servers.json`:

```json
{
  "servers": {
    "notion": {
      "command": "shannon-mcp-saas",
      "args": ["notion"],
      "env": {}
    }
  }
}
```

For headless runs (CI / scripted agent) add the token to the `env`
block instead of relying on the keyring:

```json
{
  "servers": {
    "notion": {
      "command": "shannon-mcp-saas",
      "args": ["notion"],
      "env": {
        "NOTION_TOKEN": "${env.NOTION_TOKEN}"
      }
    }
  }
}
```

---

## 4. Permission model

The Notion server integrates with the shared `server::SessionGrants`
gate that landed with P1-3 v2:

| Tool                    | `required_permission()` | Auto-granted by `with_read_only_defaults`? |
|-------------------------|-------------------------|--------------------------------------------|
| `notion_search_pages`   | `read`                  | yes                                        |
| `notion_get_page`       | `read`                  | yes                                        |
| `notion_list_databases` | `read`                  | yes                                        |
| `notion_query_database` | `read`                  | yes                                        |
| `notion_append_block`   | `write`                 | no — host must call `tools/grant` first    |
| `notion_create_page`    | `write`                 | no — host must call `tools/grant` first    |

The server's `tools/grant` JSON-RPC method is the **only** way to
unlock write scope. The `args.permission` self-attested field that
used to gate write tools has been stripped at the JSON-RPC boundary
(see `server::handle_tools_call`) — an LLM that ships
`{"permission":"write"}` on a `notion_create_page` call is rejected
with:

```
write scope not granted for tool notion_create_page; host must call tools/grant first
```

The upstream Notion token remains the actual authority for what the
API will accept; the gate above only decides whether the request
reaches Notion at all.

### 4.1 Recommended `PermissionRule` examples

`~/.shannon/policies.toml`:

```toml
# Default — reads only, no writes.
[[rules]]
match   = "notion_*"
effect  = "allow"
scope   = "read"

# Explicitly deny any write tool until `tools/grant` is called.
[[rules]]
match   = "notion_append_block"
effect  = "deny"
message = "append_block requires an interactive `tools/grant` call first"

[[rules]]
match   = "notion_create_page"
effect  = "deny"
message = "create_page requires an interactive `tools/grant` call first"
```

---

## 5. Tool contracts (schemas + return shapes)

All six tools return a JSON object whose top-level keys are stable —
downstream code can key off `page`, `block`, `rows`, etc. without
inspecting Notion's wire format. The `properties` and `block` fields
are passed through verbatim because the schema is dynamic per
database.

### 5.1 `notion_search_pages` (read)

Input:

```json
{
  "query":        "deploy",                                          // optional
  "filter":       { "value": "page", "property": "object" },        // optional
  "sort":         { "direction": "descending", "timestamp": "last_edited_time" }, // optional
  "start_cursor": "…",                                               // optional
  "page_size":    50                                                 // optional, 1-100
}
```

Returns `{ "results": [ … ], "has_more": bool, "next_cursor": … }`.
Each entry is a Notion page **or** database object, depending on
`filter`. If `query` is omitted, Notion returns the most recently
edited objects the integration can see.

### 5.2 `notion_get_page` (read)

Input:

```json
{
  "page_id": "00000000-0000-0000-0000-000000000000"
}
```

Returns `{ "page": { id, object, created_time, last_edited_time, archived, properties, url } }`.
The `properties` object is the database-specific property map; each
value's shape depends on the column type (`title`, `rich_text`,
`select`, `date`, etc.).

### 5.3 `notion_append_block` (write)

Input:

```json
{
  "page_id": "00000000-0000-0000-0000-000000000000",
  "block": {
    "object":    "block",
    "type":      "paragraph",
    "paragraph": { "rich_text": [{ "type": "text", "text": { "content": "Hello, world." } }] }
  }
}
```

Returns `{ "block": { id, object, type, has_children }, "appended": true }`.
The tool wraps the supplied `block` in a `children: [block]` array
and `PATCH /v1/blocks/{page_id}/children`, so the single-block
contract is preserved. Rich trees (paragraph with nested `children`)
are supported by passing a block that itself contains a `children`
key — Notion flattens the array server-side.

### 5.4 `notion_create_page` (write)

Input:

```json
{
  "parent":     { "database_id": "00000000-0000-0000-0000-000000000000" },
  "properties": { "Name": { "title": [{ "text": { "content": "My new page" } }] } },
  "children":   [ /* optional initial blocks */ ]
}
```

`parent` may also be `{ "page_id": "…" }` to create a sub-page under
an existing page (in which case the only `properties` key is `title`).
Returns `{ "page": { … }, "created": true }`.

### 5.5 `notion_list_databases` (read)

Input:

```json
{
  "start_cursor": "…",   // optional
  "page_size":    50     // optional, 1-100
}
```

Returns `{ "databases": [ … ], "has_more": bool, "next_cursor": … }`.
Internally the server composes `POST /v1/search` with
`filter: { value: "database", property: "object" }` because Notion
does not expose a dedicated list-databases endpoint.

### 5.6 `notion_query_database` (read)

Input:

```json
{
  "database_id":  "00000000-0000-0000-0000-000000000000",
  "filter":       { "property": "Status", "select": { "equals": "In progress" } },
  "sorts":        [{ "property": "Last edited", "direction": "descending" }],
  "start_cursor": "…",
  "page_size":    25
}
```

Returns `{ "rows": [ … ], "has_more": bool, "next_cursor": … }`. Each
`rows[]` entry is a full page object (same shape as `notion_get_page`).
`filter` and `sorts` accept any Notion-shape object — see
[Querying a database](https://developers.notion.com/reference/post-database-query)
in Notion's docs for the full grammar.

---

## 6. Rate limiting

Notion advertises ~3 requests per second per integration. The client
honours `Retry-After` exactly and falls back to exponential backoff
(1s × 2^attempt, capped at `MAX_RETRIES=3`) on 5xx and 429. A 401 is
treated as terminal — the token is bad and no amount of retry will
help; the caller must re-authenticate.

For sustained-throughput integrations, see [Notion's rate-limit
guidance](https://developers.notion.com/reference/request-limits) and
consider batching reads with `query_database` over a large
`page_size` (max 100) rather than per-page calls.

---

## 7. Troubleshooting

### 7.1 401 — token revoked

```
upstream API error: unauthorized (401) — Notion token revoked or integration removed from the workspace
```

The `NOTION_TOKEN` is no longer valid. Either the secret was
regenerated in the Notion UI (revoking the old one) or the
integration was deleted. Re-create the integration in
`Settings → Integrations`, copy the new secret, and re-paste it via
`connections connect notion` (or `export NOTION_TOKEN=…` for headless
runs).

### 7.2 404 — page not shared with the integration

```
upstream API error: not found (404) — /pages/00000000-…
```

Notion returns 404 when the page exists but has not been explicitly
shared with the integration. Open the page in Notion, click
`… → Connections → <integration name> → Confirm`, then retry. The
error string includes the requested path so you can map it back to
the original tool call.

### 7.3 403 — integration lacks scope

```
upstream API error: forbidden (403) — integration lacks access to the requested page/database: {"object":"error","code":"unauthorized","message":"…"}
```

Some Notion errors come back as 403 even when the cause is a missing
share. If the body says `object_not_found` and §7.2 doesn't apply,
the page may live in a workspace the integration is not invited to.
Re-create the integration in the correct workspace and re-share the
target pages.

### 7.4 429 — backoff

```
upstream API error: rate-limited; retry after Ns
```

The server retried `MAX_RETRIES` times with exponential backoff and
the upstream kept responding 429. Either slow down the call rate or
batch reads (§6). The `retry_after_secs` field is the last `Retry-After`
value the server saw — respect it before retrying manually.

### 7.5 `NotionTokenShape` introspection

The `NotionTokenShape` struct in `notion::auth` exists so future
telemetry or rotation checks can read the raw value (e.g. to confirm
the prefix is `ntn_` vs the legacy `secret_`). It is not wired into
the request path; the raw `Token` is sent as-is.

---

## 8. Module layout

```
crates/shannon-mcp-saas/src/notion/
├── mod.rs     # submodule declarations + tests
├── auth.rs    # Token (redacted Bearer), TokenProvider, keyring constants
├── api.rs     # NotionClient: 6 REST methods + 401/403/404/429/5xx handling
├── tools.rs   # 6 McpTool impls + all_tools() / all_tools_unauth()
└── tests.rs   # mockito HTTP coverage; ~24 cases
```

The shape mirrors `slack/` and `jira/` so future contributors can
diff them side-by-side. Notion is the simplest of the three: no OAuth
flow, no PKCE, no cloudid resolution — the auth path is `Token →
Bearer header → request`.
