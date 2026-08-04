# Linear MCP server (P1-3 v5)

Shannon ships a dedicated Linear MCP server that exposes five tools over
stdio JSON-RPC 2.0. Linear uses GraphQL at `https://api.linear.app/graphql`
and authenticates via personal API keys (no OAuth on the Shannon side).

| Tool                      | Scope | Notes                                                |
|---------------------------|-------|------------------------------------------------------|
| `linear_list_issues`      | read  | Paginated issue list with optional GraphQL filter    |
| `linear_get_issue`        | read  | Single issue by UUID or `ENG-123` identifier         |
| `linear_create_issue`     | write | Create issue in a team                               |
| `linear_update_status`    | write | Move issue to a workflow state by state id           |
| `linear_list_teams`       | read  | List teams + their workflow states (for state id resolution) |

---

## 1. Build & launch

The Linear module lives behind the `linear` Cargo feature. It is **off**
by default — the SaaS has the largest surface area of the v2–v5 modules
(GraphQL SDK, keyring wiring) and keeping it opt-in lets the default
binary stay slim.

```toml
# crates/shannon-mcp-saas/Cargo.toml
[features]
default = ["slack", "jira"]
slack = []
jira  = []
linear = []
```

Build with the feature and launch:

```bash
cargo build -p shannon-mcp-saas --release --features linear
./target/release/shannon-mcp-saas linear
# prints the registered tool listing to **stderr**, then enters the
# stdio JSON-RPC loop on stdin/stdout.
```

The host (Shannon CLI / desktop) spawns the process and pipes JSON-RPC
frames through its stdin. On `initialize` the server returns the five
Linear tools plus the standard MCP `serverInfo`.

---

## 2. Configuration

### 2.1 Pick an auth method

Linear has no OAuth flow for personal API keys, so Shannon ships one
auth path:

| Method                              | When to use                              | Storage                                       |
|-------------------------------------|------------------------------------------|-----------------------------------------------|
| **Personal API key** (`lin_api_…`)  | Interactive desktop / headless / CI      | `LINEAR_TOKEN` env var or keyring `shannon-mcp-saas` / `linear-token` |

`TokenProvider::get_token()` resolves in that priority order:
`LINEAR_TOKEN` env → keyring entry.

### 2.2 Personal API key

1. Sign in to <https://linear.app>.
2. Click your avatar → **Settings** → **API** → **Personal API keys**.
3. Click **Create key**, give it a label (e.g. "shannon-mcp"),
   copy the resulting `lin_api_…` string.

Then either export it as an env var:

```bash
export LINEAR_TOKEN="lin_api_1a2b3c4d..."
shannon-mcp-saas linear
```

or persist to the keyring via the desktop `connections` UX
("Add Linear" → paste token). The binary will read the keyring
on every boot — no env var needed in the process tree.

### 2.3 Keyring layout

| Service            | Account          | Contents                  |
|--------------------|------------------|---------------------------|
| `shannon-mcp-saas` | `linear-token`   | Personal API key (`lin_api_…`) |

The desktop `connections` view surfaces "Add Linear" / "Sign out" backed
by these entries. Removing them forces the next boot back into the
unauthenticated state.

### 2.4 Environment variables

| Variable        | Required for | Notes                                       |
|-----------------|--------------|---------------------------------------------|
| `LINEAR_TOKEN`  | personal API key | Token from <https://linear.app/settings/api> |

### 2.5 Multi-workspace support

Linear is multi-workspace — one personal API key works for only one
workspace. The current SaaS single-tenants to that workspace. Future
work can extend `TokenProvider::account_hint` into per-workspace
keyring accounts (`linear-token:<workspace>`).

---

## 3. Permission model

The Linear server integrates with the shared `server::SessionGrants`
gate that landed with P1-3 v2:

| Tool                      | `required_permission()` | Auto-granted by `with_read_only_defaults`? |
|---------------------------|-------------------------|--------------------------------------------|
| `linear_list_issues`      | `read`                  | yes                                        |
| `linear_get_issue`        | `read`                  | yes                                        |
| `linear_list_teams`       | `read`                  | yes                                        |
| `linear_create_issue`     | `write`                 | no — host must call `tools/grant` first    |
| `linear_update_status`    | `write`                 | no — host must call `tools/grant` first    |

The server's `tools/grant` JSON-RPC method is the **only** way to unlock
write scope. The `args.permission` self-attested field that used to
gate write tools has been stripped at the JSON-RPC boundary (see
`server::handle_tools_call`) — an LLM that ships `{"permission":"write"}`
on a `linear_create_issue` call is rejected with:

```
write scope not granted for tool linear_create_issue; host must call tools/grant first
```

The upstream API key remains the actual authority for what Linear will
accept; the gate above only decides whether the request reaches Linear
at all.

---

## 4. Tool contracts (schemas + return shapes)

### 4.1 `linear_list_issues` (read)

Input:

```json
{
  "filter": { "state": { "type": { "in": "started" } } },
  "first": 50,           // optional, 1–100
  "after": "cursor-xyz"  // optional, from previous response
}
```

The `filter` argument is a free-form GraphQL `IssueFilter` object —
Linear accepts any of its nested shapes (state, team, priority, label,
assignee, etc.). Pass it verbatim.

Returns:

```json
{
  "issues": [
    {
      "id": "abc-1",
      "identifier": "ENG-1",
      "title": "first issue",
      "priority": 2.0,
      "state": { "id": "state-1", "name": "In Progress", "type": "started" },
      "team": { "id": "team-1", "key": "ENG", "name": "Engineering" }
    }
  ],
  "page_info": { "hasNextPage": false, "endCursor": null }
}
```

### 4.2 `linear_get_issue` (read)

Input:

```json
{ "id": "ENG-123" }
```

`id` may be a UUID (`abc-…`) or a human-readable identifier
(`ENG-123`). Returns:

```json
{ "issue": { ...same shape as items in linear_list_issues... } }
```

Missing identifiers come back as a GraphQL error: `{"errors":[{"message":"Entity not found","path":["issue"]}]}` — the tool surfaces `errors[].message` directly.

### 4.3 `linear_create_issue` (write)

Input:

```json
{
  "title": "Spike GraphQL error handling",
  "team_id": "team-uuid-from-list-teams",
  "description": "Optional markdown body.",
  "priority": 2.0,
  "label_ids": ["label-uuid-1", "label-uuid-2"]
}
```

`priority` follows Linear's `0..4` scale
(0 = No priority, 1 = Urgent, 2 = High, 3 = Medium, 4 = Low).

Returns:

```json
{ "issue": { ...same shape as linear_get_issue... }, "created": true }
```

### 4.4 `linear_update_status` (write)

Input:

```json
{
  "issue_id": "issue-uuid",
  "state_id": "state-uuid"
}
```

`state_id` must be a workflow state UUID — **not** a name. Discover
state IDs via `linear_list_teams` first (see §4.5 and §5). Returns:

```json
{ "issue": { ...same shape... }, "updated": true }
```

### 4.5 `linear_list_teams` (read)

Input:

```json
{ "first": 50 }  // optional
```

Returns:

```json
{
  "teams": [
    {
      "id": "team-uuid",
      "key": "ENG",
      "name": "Engineering",
      "description": null,
      "states": [
        { "id": "state-uuid-1", "name": "Todo",        "type": "unstarted" },
        { "id": "state-uuid-2", "name": "In Progress", "type": "started" },
        { "id": "state-uuid-3", "name": "Done",        "type": "completed" }
      ]
    }
  ]
}
```

The state UUIDs in this payload are what `linear_update_status` needs.
The discoverability step is unavoidable — Linear workflow states are
per-team UUIDs, not strings, so callers always resolve
`name → id` first.

---

## 5. Usage examples

### 5.1 List unstarted issues in a team

```bash
# using the stdio JSON-RPC directly
echo '{"jsonrpc":"2.0","id":"1","method":"initialize","params":{}}' | \
  LINEAR_TOKEN="lin_api_..." ./shannon-mcp-saas linear
# then a tools/call frame:
#   tools/call { name: "linear_list_issues", arguments: {
#       "filter": { "state": { "type": { "eq": "unstarted" } }, "team": { "id": { "eq": "TEAM-UUID" } } },
#       "first": 50 }}
```

From the Shannon REPL:

```text
> /tool linear_list_issues filter={"state":{"type":{"eq":"unstarted"}}} first=25
```

### 5.2 Create + move an issue

```text
# 1) resolve state ids via linear_list_teams (run once per workspace)
> /tool linear_list_teams

# 2) create the issue in team "Engineering"
> /tool linear_create_issue title="Spike: error mapping" team_id=<team-uuid>

# 3) move it to "In Progress"
> /tool linear_update_status issue_id=<new-issue-uuid> state_id=<in-progress-state-uuid>
```

### 5.3 Look up an existing issue

```text
> /tool linear_get_issue id=ENG-1234
```

Returns the full `Issue` shape (id, identifier, title, description,
priority, state, team, plus URLs / timestamps).

---

## 6. Rate limiting

Linear publishes a soft limit of `1500 req/hr` for personal API keys
(<https://developers.linear.app/docs/graphql-api>). The Shannon client
implements the standard `Retry-After` honour path used by every other
SaaS module:

| Signal                                  | Behaviour |
|-----------------------------------------|-----------|
| `HTTP 429` + `Retry-After: <secs>`      | Sleep exactly that long, then retry. Up to 3 retries, then surface `ApiError::RateLimited`. |
| `HTTP 429` (no `Retry-After`)           | Exponential backoff — 1s, 2s, 4s, capped at 60s. |
| `HTTP 5xx`                              | Same exponential backoff as 429-without-header. |
| `HTTP 401`                              | Not retried — token revoked. Surface `ApiError::Unauthorized`; the host prompts the user to re-enter the key. |
| `{ "errors": [...] }` on 200            | Surfaced as `ApiError::GraphQL(<joined messages>)`. Each error is a JSON object with at least `{ message, path?, extensions? }`; we join the messages with `"; "`. |

Backoff is implemented via `tokio::time::sleep`. Tests set
`with_backoff_scale(0)` to make retry behaviour instant. Production
uses the default scale.

---

## 7. Troubleshooting

| Symptom                                                          | Likely cause                                                                |
|------------------------------------------------------------------|------------------------------------------------------------------------------|
| `no Linear token configured: set LINEAR_TOKEN or save one via TokenProvider` at startup | No env var and no keyring entry — open the desktop `connections` UX and add one. |
| `unauthorized (401)` mid-session                                 | Token revoked in Linear — re-create the key and update the keyring. |
| `GraphQL error: Authentication required`                         | Header missing — usually a `LINEAR_TOKEN` mismatch (truncated token?). |
| `rate-limited; retry after Ns` and the call doesn't retry past 4 attempts | Linear hit; wait for the `Retry-After` window or reduce parallel calls. |
| `GraphQL error: Entity not found` on `linear_get_issue`          | Identifier typo (`ENG-1234` vs `ENG-123`). Verify via `linear_list_issues` first. |
| `argument Validation Error: …` on `linear_create_issue`          | Field shape wrong (e.g. `priority` out of range, label UUID doesn't exist). |
| `state_id rejected` on `linear_update_status`                   | The state belongs to a different team. Re-run `linear_list_teams` and pick the UUID from the correct team. |
| `server error: HTTP 502 / 503 / 504`                             | Linear's gateway is degraded. The client backs off automatically; on persistent 5xx, the call eventually surfaces as `ApiError::Server`. |

If the keyring is locked (e.g. Linux Secret Service daemon not running),
`TokenProvider::get_token()` returns `AuthError::Keyring` which the
server translates into `ApiError::Unauthorized`-shaped output on the
first upstream call.

---

## 8. GraphQL notes

### 8.1 Operation shape

Every Linear call is a single `POST /graphql` with body:

```json
{ "query": "query ListIssues(...) { issues(first: $first, after: $after, filter: $filter) { ... } }",
  "variables": { "first": 50, "after": "cursor", "filter": { ... } } }
```

Responses are always `{ "data": T | null, "errors": [...] }`. The
client surfaces `errors[].message` directly; the `path` field is kept
on the parsed struct for future debug logging.

### 8.2 Discoverability for state transitions

`linear_update_status` requires a state UUID, **not** a name. Names
are scoped per team and can collide (`"Done"` appears in every team).
The Shannon client does not maintain a local state cache — refresh via
`linear_list_teams` whenever the workspace's workflow has changed.
A future iteration could expose a `resolve_state_name(team_id, name)` shape
that hides the round-trip; for now the user-facing call is explicit
about the two-step.

### 8.3 Variable typing

`priority` is passed through as a JSON number; Linear accepts integers
on the wire so we serialize via `serde_json::Number::from_f64`. Non-integer
values silently round to 0 — Linear rejects with
`Argument Validation Error: priority must be an integer` if you pass
something exotic. Stick to `0..4`.

### 8.4 Pagination

`linear_list_issues` returns a `PageInfo` cursor (`hasNextPage`,
`endCursor`). Pass `endCursor` back as `after` for the next page.
`linear_list_teams` does not paginate because Shannon's expected
workspace size fits in a single page, but the query accepts a `first`
clamp (default 50, max 100) so test fixtures stay deterministic.

### 8.5 No attachments, no comments (yet)

The 5-tool surface intentionally excludes `attachmentCreate` /
`commentCreate` — both add a separate file-upload pipeline and a
comment thread object, neither of which makes sense without a UI
round-trip. They can be added in a follow-up P1-3 v6 module without
breaking this contract.

---

## 9. Tests

The crate includes:

- ~24 Linear-specific tests in
  `crates/shannon-mcp-saas/src/linear/tests.rs` plus api/auth/tools
  unit tests (happy paths + 401 / 429 / 5xx / GraphQL-error edges +
  filter / cursor round-trips for each tool).
- 4 server-level Linear tests in `server::tests`:
  `linear_tools_list_advertises_five_tools`,
  `linear_read_tools_pre_granted_write_tools_denied`,
  `linear_write_tool_without_grant_is_denied_even_if_args_permission_set`,
  `linear_write_tool_succeeds_after_tools_grant`.

Run them locally:

```bash
cargo nextest run -p shannon-mcp-saas --features linear linear::
```

CI strict gate:

```bash
cargo fmt --all -- --check
cargo clippy -p shannon-mcp-saas --features linear --all-targets -- -D warnings
cargo nextest run -p shannon-mcp-saas --features linear
```

---

## 10. Files added / modified

```
crates/shannon-mcp-saas/Cargo.toml                  # `linear` feature
crates/shannon-mcp-saas/src/lib.rs                  # gate `pub mod linear`
crates/shannon-mcp-saas/src/main.rs                 # `linear` subcommand in dispatch
crates/shannon-mcp-saas/src/server.rs               # 4 server-level Linear tests
crates/shannon-mcp-saas/src/linear/mod.rs           # module entry + `LINEAR` const
crates/shannon-mcp-saas/src/linear/auth.rs          # API-key + keyring + env var fallback
crates/shannon-mcp-saas/src/linear/api.rs           # GraphQL client + rate-limit backoff
crates/shannon-mcp-saas/src/linear/tools.rs         # 5 MCP tools + ServerTool adapter
crates/shannon-mcp-saas/src/linear/tests.rs         # mockito-backed coverage
docs/integrations/linear.md                         # this file
```

[linear-graphql]: https://developers.linear.app/docs/graphql-api
[linear-settings]: https://linear.app/settings/api
