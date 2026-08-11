# Slack MCP server (P1-3 v2)

Shannon ships a dedicated Slack MCP server that exposes six tools over
stdio JSON-RPC 2.0:

| Tool                       | Scope | Notes                                       |
|----------------------------|-------|---------------------------------------------|
| `slack_post_message`       | write | Posts a top-level message to a channel      |
| `slack_search_messages`    | read  | Searches public/private messages by query   |
| `slack_read_channel`       | read  | Reads recent history for a channel          |
| `slack_thread_reply`       | write | Replies to a thread (sets `thread_ts`)      |
| `slack_list_channels`      | read  | Lists channels visible to the bot          |
| `slack_get_user_info`      | read  | Looks up a user by id                       |

---

## 1. Build & launch

The Slack module lives behind the `slack` Cargo feature (in the default
feature set alongside `jira`):

```toml
# crates/shannon-mcp-saas/Cargo.toml
[features]
default = ["slack", "jira"]
slack = []
jira  = []
```

Build the binary and launch the Slack backend:

```bash
cargo build -p shannon-mcp-saas --release
./target/release/shannon-mcp-saas slack
# prints the registered tool listing to **stderr**, then enters the
# stdio JSON-RPC loop on stdin/stdout.
```

The host (Shannon CLI / desktop) spawns the process and pipes JSON-RPC
frames through its stdin. On `initialize` the server returns the six
Slack tools plus the standard MCP `serverInfo`.

---

## 2. Configuration

### 2.1 Pick an auth method

Two paths, both supported out of the box:

| Method           | When to use                              | Storage                                       |
|------------------|------------------------------------------|-----------------------------------------------|
| **Bot token** (`xoxb-…`) | Headless / CI / scripted agent runs | `SLACK_BOT_TOKEN` env var or keyring `slack/bot-token` |
| **OAuth 2.0 (browser)** | Interactive desktop / CLI sessions   | keyring `slack/bot-token` (+ `slack/refresh-token`)    |

`TokenProvider::get_token()` resolves them in that priority order:
bot-token env → keyring `slack/bot-token`.

### 2.2 Bot token (Simple)

```bash
export SLACK_BOT_TOKEN="xoxb-..."
shannon-mcp-saas slack
```

Or persist via the keyring (no env vars in the process tree):

```bash
# The desktop `connections connect slack` UX writes the token to the
# keyring so the launcher doesn't need env vars in argv.
```

### 2.3 OAuth 2.0 (browser)

Configure a Slack app at <https://api.slack.com/apps>:

- **Redirect URL**: `http://127.0.0.1:<ephemeral>/callback` (the server
  binds `127.0.0.1:0` so the OS picks a free port — record the assigned
  port from the JSON-RPC `bind_addr` field once the flow starts).
- **Bot token scopes**:
  - `channels:read`     — list channels (tool: `slack_list_channels`)
  - `channels:history`  — read recent messages (tool: `slack_read_channel`)
  - `chat:write`        — post messages + thread replies
  - `users:read`        — look up users by id (tool: `slack_get_user_info`)
  - `search:read`       — workspace search (tool: `slack_search_messages`)

Pass `client_id` and `client_secret` to the OAuth flow runner at
startup. The flow:

1. opens `https://slack.com/oauth/v2/authorize?...` in the system browser
2. the user grants the requested scopes
3. Slack redirects to the local callback (`127.0.0.1:<port>/callback`)
4. the callback server validates the CSRF state (constant-time, no
   logging of expected/actual), exchanges `code` for an access token at
   `https://slack.com/api/oauth.v2.access`
5. persists the access token (and refresh token when present) to the OS
   keyring (`service=shannon`, accounts `slack/bot-token` and
   `slack/refresh-token`)

### 2.4 Keyring layout

All credentials live in the OS keyring:

| Service     | Account               | Contents                |
|-------------|-----------------------|-------------------------|
| `shannon`   | `slack/bot-token`     | Bot token (`xoxb-…`)    |
| `shannon`   | `slack/refresh-token` | Refresh token (OAuth)   |

The desktop `connections` view surfaces "Add Slack" / "Sign out" backed
by these entries. Removing them forces the next boot back into the
unauthenticated state.

### 2.5 Environment variables

| Variable           | Required for | Notes                                       |
|--------------------|--------------|---------------------------------------------|
| `SLACK_BOT_TOKEN`  | bot token    | Slack bot token from your app              |

OAuth does not require env vars — the client id/secret are wired in via
the host config.

---

## 3. Permission model

The Slack server integrates with the shared `server::SessionGrants` gate
that landed with P1-3 v2:

| Tool                      | `required_permission()` | Auto-granted by `with_read_only_defaults`? |
|---------------------------|-------------------------|--------------------------------------------|
| `slack_search_messages`   | `read`                  | yes                                        |
| `slack_read_channel`      | `read`                  | yes                                        |
| `slack_list_channels`     | `read`                  | yes                                        |
| `slack_get_user_info`     | `read`                  | yes                                        |
| `slack_post_message`      | `write`                 | no — host must call `tools/grant` first    |
| `slack_thread_reply`      | `write`                 | no — host must call `tools/grant` first    |

The server's `tools/grant` JSON-RPC method is the **only** way to unlock
write scope. The `args.permission` self-attested field that used to
gate write tools has been stripped at the JSON-RPC boundary (see
`server::handle_tools_call`) — an LLM that ships `{"permission":"write"}`
on a `slack_post_message` call is rejected with:

```
write scope not granted for tool slack_post_message; host must call tools/grant first
```

The upstream bot token remains the actual authority for what Slack's
API will accept; the gate above only decides whether the request reaches
Slack at all.

---

## 4. Tool contracts (schemas + return shapes)

### 4.1 `slack_post_message` (write)

Input:

```json
{
  "channel": "C01234",
  "text": "Hello, world."
}
```

Returns `{ "message": { "ts": "...", "channel": "..." }, "posted": true }`.

### 4.2 `slack_search_messages` (read)

Input:

```json
{
  "query": "deploy",
  "limit": 20    // optional
}
```

Returns `{ "matches": [ ... ], "total": N }`.

### 4.3 `slack_read_channel` (read)

Input:

```json
{
  "channel": "C01234",
  "limit": 50       // optional, 1-200
}
```

Returns `{ "messages": [ ... ] }`. Each message carries `ts`, `user`,
`text`, and (if present) `thread_ts`.

### 4.4 `slack_thread_reply` (write)

Input:

```json
{
  "channel": "C01234",
  "thread_ts": "1700000000.000100",
  "text": "Agree."
}
```

Returns `{ "message": { "ts": "...", "channel": "..." }, "posted": true }`.

### 4.5 `slack_list_channels` (read)

Input: `{}` (no required args). Optional `cursor` + `limit` for paging.

Returns `{ "channels": [ ... ] }`.

### 4.6 `slack_get_user_info` (read)

Input:

```json
{ "user_id": "U01234" }
```

Returns `{ "user": { "id": "U01234", "name": "alice", "real_name": "Alice A" } }`.

---

## 5. Rate-limit handling

Slack surfaces `Retry-After` on `HTTP 429` and Slack-tier rate-limit
errors in the envelope (`{ "ok": false, "error": "ratelimited", ... }`).
The client honours both:

| Signal                                  | Behaviour |
|-----------------------------------------|-----------|
| `HTTP 429` + `Retry-After: <secs>`      | Sleep exactly that long, then retry. Up to 3 retries, then surface `ApiError::RateLimited`. |
| `HTTP 429` (no `Retry-After`)           | Exponential backoff — 1s, 2s, 4s, capped at 60s. |
| `HTTP 5xx`                              | Same exponential backoff as 429-without-header. |
| `HTTP 401`                              | Not retried — token revoked. The host should re-authorize. |
| `X-RateLimit-Reset` on 2xx              | Sleep up to `reset` (capped at 60s) before returning so the next call lands in a fresh budget. |
| `{ "ok": false, "error": "..." }`        | Surfaced as `ApiError::Slack(<error>)` — Slack returns these on a 200 status for soft errors (`missing_scope`, `channel_not_found`, `not_in_channel`, etc.). |

Backoff is implemented via `tokio::time::sleep`. Tests set
`with_backoff_scale(0)` to make retry behaviour instant. Production
uses the default scale.

---

## 6. Tests

The crate includes:

- 32 Slack-specific tests in `crates/shannon-mcp-saas/src/slack/tests.rs`
  plus api/auth/tools unit tests (happy path + rate-limited + auth for
  each tool, error-surface checks).
- 4 server-level Slack tests in `server::tests`:
  `slack_tools_list_advertises_six_tools`,
  `slack_read_tools_pre_granted_write_tools_denied`,
  `slack_write_tool_without_grant_is_denied_even_if_args_permission_set`,
  `slack_write_tool_succeeds_after_tools_grant`.

Run them locally:

```bash
cargo nextest run -p shannon-mcp-saas --no-default-features --features slack slack::
```

CI strict gate:

```bash
cargo fmt --all -- --check
cargo clippy --no-default-features --features slack -p shannon-mcp-saas --all-targets -- -D warnings
cargo nextest run -p shannon-mcp-saas --no-default-features --features slack
```

---

## 7. Files added / modified

```
crates/shannon-mcp-saas/Cargo.toml                  # `slack` feature
crates/shannon-mcp-saas/src/lib.rs                  # gate `pub mod slack`
crates/shannon-mcp-saas/src/main.rs                 # `slack` subcommand in dispatch
crates/shannon-mcp-saas/src/server.rs               # 4 server-level Slack tests
crates/shannon-mcp-saas/src/slack/mod.rs            # module entry
crates/shannon-mcp-saas/src/slack/auth.rs           # OAuth (v2) + bot-token + keyring
crates/shannon-mcp-saas/src/slack/api.rs            # REST client + rate-limit backoff
crates/shannon-mcp-saas/src/slack/tools.rs          # six MCP tools + ServerTool adapter
crates/shannon-mcp-saas/src/slack/tests.rs          # mockito-backed coverage
docs/integrations/slack.md                          # this file
```

[slack-dev]: https://api.slack.com/apps
