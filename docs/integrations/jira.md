# Jira MCP server (P1-3 v3)

Shannon ships a dedicated Jira (Atlassian Cloud) MCP server that exposes
four tools over stdio JSON-RPC 2.0:

| Tool                  | Scope | Notes                                  |
|-----------------------|-------|----------------------------------------|
| `jira_search_issues`  | read  | JQL search; returns `{ issues, total }` |
| `jira_get_issue`      | read  | Returns the full v3 issue JSON         |
| `jira_create_issue`   | write | Returns `{ issue, created: true }`     |
| `jira_transition`     | write | Resolves a status name to a transition id, returns `{ issue, transitioned: true }` |

---

## 1. Build & launch

The Jira module lives behind the `jira` Cargo feature (currently in the
default feature set, alongside `slack`):

```toml
# crates/shannon-mcp-saas/Cargo.toml
[features]
default = ["slack", "jira"]
slack = []
jira  = []
```

Build the binary and launch the Jira backend:

```bash
cargo build -p shannon-mcp-saas --release
./target/release/shannon-mcp-saas jira
# prints the registered tool listing to **stderr**, then enters the
# stdio JSON-RPC loop on stdin/stdout.
```

The host (Shannon CLI / desktop) spawns the process and pipes JSON-RPC
frames through its stdin. On `initialize` the server returns the four
Jira tools plus the standard MCP `serverInfo`.

---

## 2. Configuration

### 2.1 Pick an auth method

Two paths, both supported out of the box:

| Method | When to use | Storage |
|--------|-------------|---------|
| **API token** (Basic auth) | Headless / CI / scripted agent runs | `JIRA_API_TOKEN` env var or keyring `jira/api-token` |
| **OAuth 2.0 (3LO, PKCE S256)** | Interactive desktop / CLI sessions | keyring `jira/oauth` (+ `jira/cloudid`) |

`TokenProvider::get_token()` resolves them in that priority order:
API-token env → keyring `jira/api-token` → keyring `jira/oauth`.

### 2.2 API token (Basic auth)

```bash
export JIRA_EMAIL="alice@example.com"      # the account you log into atlassian.net with
export JIRA_API_TOKEN="ATATT..."           # from id.atlassian.com/manage-profile/security/api-tokens
shannon-mcp-saas jira
```

Or persist via the keyring (no env vars in the process tree):

```bash
# The provider writes "email:token" as the keyring password.
# The shannon desktop app performs this on `connections connect jira`
# and exposes it from `tools/grant` surfaces elsewhere.
```

### 2.3 OAuth 2.0 (3LO, PKCE S256)

Configure the OAuth client in your [developer console][atlassian-dev]:

- **App type**: `OAuth 2.0 (3LO)`
- **Authorization callback URL**: `http://127.0.0.1:<ephemeral>/callback`
  (the server binds `127.0.0.1:0` so the OS picks a free port — record
  the assigned port from the JSON-RPC `bind_addr` field).
- **Permissions / scopes**:
  - `read:jira-work` — search + get issue
  - `read:jira-user` — user lookup (covers assignee on transitions)
  - `write:jira-work` — create + transition issue
  - `offline_access` — refresh-token issuance

Pass `client_id` and `client_secret` to the OAuth flow runner at
startup. The flow:

1. opens `https://auth.atlassian.com/authorize?...` in the system browser
2. the user grants the requested scopes
3. Atlassian redirects to the local callback (`127.0.0.1:<port>/callback`)
4. the callback server validates the CSRF state (constant-time, no
   logging of expected/actual), exchanges `code` for tokens at
   `https://auth.atlassian.com/oauth/token`
5. resolves the **cloudid** via
   `https://api.atlassian.com/oauth/token/accessible-resources`
   (the first returned resource is used to address the tenant-scoped
   REST endpoint: `https://api.atlassian.com/ex/jira/<cloudid>/rest/api/3/...`)
6. persists access + cloudid to the OS keyring (`service=shannon`,
   accounts `jira/oauth` and `jira/cloudid`)

The cloudid is reused on subsequent boots — we skip the
accessible-resources round-trip when it is present.

### 2.4 Keyring layout

All credentials live in the OS keyring:

| Service     | Account         | Contents                              |
|-------------|-----------------|---------------------------------------|
| `shannon`   | `jira/api-token`| `email:token` for Basic auth          |
| `shannon`   | `jira/oauth`    | Access token (3LO)                    |
| `shannon`   | `jira/cloudid`  | Atlassian cloud id (OAuth only)       |

The desktop `connections` view surfaces "Add Jira" / "Sign out" backed
by these entries. Removing them forces the next boot back into the
unauthenticated state.

### 2.5 Environment variables

| Variable          | Required for | Notes                                         |
|-------------------|--------------|-----------------------------------------------|
| `JIRA_API_TOKEN`  | API-token    | Atlassian API token from your profile        |
| `JIRA_EMAIL`      | API-token    | Email tied to the token (Basic auth username) |

OAuth does not require env vars — the client id/secret are wired in via
the host config.

---

## 3. Permission model

The Jira server integrates with the shared `server::SessionGrants` gate
that landed with P1-3 v2 (the Slack 4-MEDIUM security fixes):

| Tool                  | `required_permission()` | Auto-granted by `with_read_only_defaults`? |
|-----------------------|-------------------------|--------------------------------------------|
| `jira_search_issues`  | `read`                  | yes                                        |
| `jira_get_issue`      | `read`                  | yes                                        |
| `jira_create_issue`   | `write`                 | no — host must call `tools/grant` first    |
| `jira_transition`     | `write`                 | no — host must call `tools/grant` first    |

The server's `tools/grant` JSON-RPC method is the **only** way to unlock
write scope. The `args.permission` self-attested field that used to
gate write tools has been stripped at the JSON-RPC boundary (see
`server::handle_tools_call`) — an LLM that ships `{"permission":"write"}`
on a `jira_create_issue` call is rejected with:

```
write scope not granted for tool jira_create_issue; host must call tools/grant first
```

The upstream Bearer / Basic token remains the actual authority for what
Atlassian's API will accept; the gate above only decides whether the
request reaches Jira at all.

---

## 4. Tool contracts (schemas + return shapes)

### 4.1 `jira_search_issues` (read)

Input:

```json
{
  "jql": "project = ENG AND status != Done",
  "max_results": 50,   // optional
  "start_at": 0        // optional
}
```

Output: `{ "issues": [ ... ], "total": 1234 }`. Each `issue` carries
`id`, `key`, and a `fields` object (Atlassian v3 schema).

### 4.2 `jira_get_issue` (read)

Input: `{ "key": "ENG-123" }`.

Output: `{ "issue": { ... full v3 issue ... } }`. The raw payload is
returned whole so the host model can pick whichever fields it needs.

### 4.3 `jira_create_issue` (write)

Input:

```json
{
  "project": "ENG",
  "summary": "Implement sign-in flow",
  "issue_type": "Task",        // or "Story" / "Bug" / "Epic" / project-specific
  "description": "Plain text description; wrapped into an ADF doc"
}
```

Returns `{ "issue": { "id": "...", "key": "ENG-124" }, "created": true }`.

### 4.4 `jira_transition` (write)

Input: `{ "key": "ENG-123", "target_status": "In Progress" }`.

The tool first calls `GET /issue/{key}/transitions` to resolve the
status name to a workflow transition id (case-insensitive match), then
issues `POST /issue/{key}/transitions`. The Jira endpoint returns
**204 No Content** on success — the tool reports
`{ "issue": { "key": "ENG-123" }, "transitioned": true }`.

When the status name is not in the workflow, the call fails with an
`InvalidRequest`-shaped error that includes the rejected name so the
LLM can pivot.

---

## 5. Rate-limit handling

Jira Cloud does not document a strict rate-limit budget, but the API
emits a family of headers the client respects:

| Header                    | Behaviour |
|---------------------------|-----------|
| `X-RateLimit-Remaining`   | If `0` on a successful response, we sleep until `X-RateLimit-Reset` (capped at 60s) before returning. Subsequent calls then find a fresh budget. |
| `X-RateLimit-Reset`       | Unix-epoch seconds — used as above. |
| `Retry-After`             | Honoured on HTTP 429 (primary rate-limit) and 403 (secondary). |
| `HTTP 429`                | Backoff using `Retry-After` if present, else the default schedule. |
| `HTTP 403 + remaining=0`  | Same as 429 (some tenants emit 403 for rate-limit responses). |
| `HTTP 5xx`                | Exponential backoff — 1s, 2s, 4s, with a 60s cap. |
| `HTTP 401`                | Not retried — token revoked. The host should re-authorize. |

Backoff is implemented via `tokio::time::sleep`. Tests set
`with_backoff_scale(0)` to make retry behaviour instant. Production
uses the default scale.

---

## 6. Tests

The crate includes:

- 33 Jira-specific tests in `crates/shannon-mcp-saas/src/jira/tests.rs`
  (happy path + rate-limited + auth-required per tool).
- 4 server-level Jira tests in `server::tests`:
  `jira_tools_list_advertises_four_tools`,
  `jira_read_tools_pre_granted_write_tools_denied`,
  `jira_write_tool_without_grant_is_denied_even_if_args_permission_set`,
  `jira_write_tool_succeeds_after_tools_grant`.

Run them locally:

```bash
cargo nextest run -p shannon-mcp-saas
```

CI strict gate (matching the Slack branch):

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -p shannon-mcp-saas -- -D warnings
cargo nextest run -p shannon-mcp-saas
```

---

## 7. Files added / modified

```
crates/shannon-mcp-saas/Cargo.toml             # add `jira` feature
crates/shannon-mcp-saas/src/lib.rs             # gate `pub mod jira`
crates/shannon-mcp-saas/src/main.rs            # `jira` subcommand in dispatch
crates/shannon-mcp-saas/src/server.rs          # 4 server-level Jira tests
crates/shannon-mcp-saas/src/jira/mod.rs        # new module entry
crates/shannon-mcp-saas/src/jira/auth.rs       # OAuth (3LO + PKCE) + API token + keyring
crates/shannon-mcp-saas/src/jira/api.rs        # REST client + rate-limit backoff
crates/shannon-mcp-saas/src/jira/tools.rs      # four MCP tools + ServerTool adapter
crates/shannon-mcp-saas/src/jira/tests.rs      # mockito-backed coverage
docs/integrations/jira.md                      # this file
```

[atlassian-dev]: https://developer.atlassian.com/developer-console/
