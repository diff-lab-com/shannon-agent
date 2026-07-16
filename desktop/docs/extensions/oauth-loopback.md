# OAuth Loopback for MCP Servers (P1.5)

**Scope**: Architecture, operator setup, and manual test plan for Shannon
Desktop's one-click OAuth flow against remote MCP servers (Notion, Linear,
Slack, etc.). Intended audience: operators (who configure vendor client IDs),
developers (extending the flow), and reviewers (validating RFC compliance).

**Status**: Implementation shipped in S2 P1.4 (PRs #8, #15). This document
written in the follow-up P1.5 validation pass.

---

## 1. Architecture

Shannon Desktop drives the full OAuth 2.1 Authorization Code + PKCE flow from
the desktop binary. No backend server is required — the loopback TCP listener
*is* the redirect target.

```
┌────────────┐    1. bind 127.0.0.1:0       ┌──────────────┐
│            │ ───────────────────────────► │              │
│            │    2. open browser to        │              │
│            │       vendor authorize_url   │              │
│  Shannon   │ ───────────────────────────► │   Browser    │
│  Desktop   │                              │              │
│            │    3. user consents          │              │
│            │ ◄─────────────────────────── │              │
│            │    4. GET /callback?code=…   │              │
│  TcpListener                               │              │
│  (ephemeral│    5. parse + verify state   └──────────────┘
│   port)    │    6. POST /token (code,
│            │       code_verifier, …)
│            │ ───────────────────────────► ┌──────────────┐
│            │    7. { access_token }       │   Vendor     │
│            │ ◄─────────────────────────── │   Token EP   │
│            │                              └──────────────┘
│            │    8. write settings.json
│            │       mcpServers.<slug>-oauth
└────────────┘
```

### Code map

| Layer | File | Role |
|---|---|---|
| Tauri command | `src/extensions_commands.rs::install_mcp_oauth_loopback` | Orchestrates the 8-step flow. Accepts `vendor_slug`, returns `InstallResult`. |
| PKCE primitives | `src/extensions/oauth.rs` | `generate_code_verifier`, `code_challenge_s256`, `generate_state`, `build_authorize_url`, `parse_callback_query` |
| Installer | `src/extensions/mcp_installers.rs::OAuthRemoteMcpInstaller` | Vendor config lookup, `pkce_context()`, `authorize_url()`, `server_config()` |
| HTTP fetch | `reqwest::Client::new()` inline | Token exchange POST |
| Persistence | `extensions::write_mcp_server_config` | Appends to `~/.shannon/settings.json#mcpServers` |
| UI | `ui/src/lib/tauri-api.ts::installMcpOAuthLoopback` | Frontend wrapper |
| Featured tab | `ui/src/components/extensions/Featured.tsx` | One-click install button |

---

## 2. RFC Compliance

### RFC 7636 (PKCE)

| Requirement | Status | Code |
|---|---|---|
| Code verifier 43-128 chars, unreserved set | ✅ | `oauth.rs:27` — 64 chars from `[A-Za-z0-9-._~]` |
| S256 challenge method | ✅ | `oauth.rs:41` — `BASE64URL_NOPAD(SHA256(verifier))` |
| Verifier sent on token exchange | ✅ | `extensions_commands.rs:301` |
| RFC 7636 §B test vector passes | ✅ | `oauth.rs:256-260` — known-good vector |

### RFC 6749 §3.1.2.4 (Native App Loopback)

| Requirement | Status | Code |
|---|---|---|
| Redirect URI uses `127.0.0.1` (not `localhost`) | ✅ | `extensions_commands.rs:233` |
| Ephemeral port assigned by OS | ✅ | `:0` bind at `extensions_commands.rs:226` |
| State parameter for CSRF | ✅ | 32 random chars, verified at `oauth.rs:166` |

### RFC 6749 §4.1.3 (Token Exchange)

| Requirement | Status | Code |
|---|---|---|
| `grant_type=authorization_code` | ✅ | `extensions_commands.rs:297` |
| Includes `redirect_uri` | ✅ | `extensions_commands.rs:299` |
| Includes `code_verifier` | ✅ | `extensions_commands.rs:301` |

---

## 3. Operator Setup

The flow requires **per-vendor client IDs**. Shannon Desktop does not ship
with embedded client IDs — each operator (self-hoster) registers their own.

### 3.1 Environment variables

Client IDs are read from environment variables at runtime. The variable name
is defined per-vendor in `src/extensions/catalog.rs::featured_vendors()` as
`client_id_env`.

To set them when launching Shannon Desktop:

```bash
# Linux/macOS
export NOTION_OAUTH_CLIENT_ID="your-client-id"
export LINEAR_OAUTH_CLIENT_ID="your-client-id"
shannon-desktop &

# Windows (PowerShell)
$env:NOTION_OAUTH_CLIENT_ID = "your-client-id"
Start-Process shannon-desktop.exe
```

If the env var is unset, `client_id` falls back to `"shannon-desktop"` at
`extensions_commands.rs:292` — this only works for vendors that pre-registered
that literal string.

### 3.2 Vendor setup guides

#### Notion

1. Go to https://www.notion.so/profile/integrations
2. **Create new integration** → type: **Public integration**
3. Set **Redirect URI** to `http://127.0.0.1:PORT/callback` where `PORT` is
   any port (Shannon binds ephemeral — Notion allows wildcard `127.0.0.1`
   per https://developers.notion.com/docs/authorization#redirect-and-state)
4. Copy the **OAuth client ID** → `NOTION_OAUTH_CLIENT_ID`
5. Required scopes: `read` (MCP server is read-only today)

> Notion does **not** issue client secrets for public integrations. PKCE is
> required, which Shannon provides.

#### Linear

1. Go to https://linear.app/settings/api → **OAuth2**
2. **Create application**
3. **Redirect URL**: `http://127.0.0.1:PORT/callback` (Linear allows loopback
   wildcards)
4. Copy **Client ID** → `LINEAR_OAUTH_CLIENT_ID`
5. Required scopes: `read`, `issues:read`, `projects:read`

#### Slack (future)

Slack requires a signed client secret and does not allow truly public OAuth
clients. This vendor is **not supported** by the current loopback flow — it
needs Phase 2 confidential-client support (see §6).

---

## 4. Manual Test Plan

Automated end-to-end tests require real OAuth client IDs and are not in CI.
Run this checklist before tagging a release that touches `extensions/oauth.rs`
or `extensions_commands.rs::install_mcp_oauth_loopback`.

### 4.1 Prerequisites

- A Notion integration with known client_id (see §3.2)
- Notion account with at least one shared page
- `NOTION_OAUTH_CLIENT_ID` env var set

### 4.2 Happy path

1. Launch Shannon Desktop with `NOTION_OAUTH_CLIENT_ID` set
2. Open Extensions Hub → **Featured** tab
3. Click **Connect** on the Notion card
4. System browser opens to `https://api.notion.com/v1/oauth/authorize?…`
5. Select the Notion workspace → **Allow access**
6. Browser shows **"Authorization received"** page
7. Shannon UI shows **Connected** badge on the Notion card
8. Verify `~/.shannon/settings.json` contains:
   ```json
   "mcpServers": {
     "notion-oauth": {
       "url": "https://mcp.notion.com/mcp",
       "headers": { "Authorization": "Bearer ntn_…" },
       "shannon:transport": "oauth_remote"
     }
   }
   ```

### 4.3 Failure modes

| Scenario | Expected behavior |
|---|---|
| User closes browser before consenting | After 5 min, Shannon shows "timeout waiting for OAuth callback" toast |
| User clicks **Cancel** on vendor consent page | Browser redirects with `?error=access_denied`; Shannon shows vendor error message |
| Attacker intercepts and modifies `state` | Shannon rejects with "state mismatch — possible CSRF attempt" |
| Token endpoint returns 401 | Shannon shows "token exchange failed (401 Unauthorized): …" |
| Loopback port already in use | OS assigns different ephemeral port; no user-facing error |
| User retries OAuth within 30s of prior success | Works — each invocation creates a new listener + new state |

---

## 5. Security Considerations

### 5.1 Plaintext token storage (known limitation)

**Current**: `access_token` is written to `~/.shannon/settings.json` as
plaintext in the `headers.Authorization` field.

**Why**: The MCP server config schema (shared with Claude Code / Anthropic's
MCP spec) stores auth in JSON. Keychain migration requires schema changes.

**Risk**: Anyone with read access to `~/.shannon/settings.json` (e.g., a
rogue process running as the user) can steal the token.

**Mitigation**: Shannon Desktop inherits the OS file permissions on
`~/.shannon/` (typically 0700 on Unix). The threat model assumes the user's
account is not compromised.

**Future**: Phase 2 will migrate access tokens to OS keychain
(`tauri-plugin-stronghold` or `keyring` crate) with a surrogate key in
settings.json.

### 5.2 No refresh token handling

Most vendors return `refresh_token` alongside `access_token`. Shannon
**discards** the refresh token — the current `TokenResponse` struct only
deserializes `access_token` (`extensions_commands.rs:313`).

**Impact**: Access tokens typically expire in ~1 hour. After expiry, the MCP
server returns 401 and the user must re-run the OAuth flow.

**Future**: Phase 2 will capture `refresh_token` + `expires_at`, store in
keychain, and add a background refresh task.

### 5.3 No client_secret support

Shannon is a **public client** (PKCE-only, no secret). Vendors that require
a client secret (Slack, GitHub OAuth Apps in confidential mode) cannot use
this flow. They must be added as MCP servers via the manual stdio form
instead.

### 5.4 Listener lifecycle

The TcpListener is bound for at most 5 minutes. If the user closes Shannon
Desktop while the OAuth flow is in-flight, the listener is dropped when the
process exits — the ephemeral port returns to the OS pool immediately. No
orphan listeners are possible.

---

## 6. Future Work (Phase 2 and beyond)

| Gap | Priority | Effort |
|---|---|---|
| Keychain token storage | P2 | 1 week |
| Refresh token capture + background refresh | P2 | 1 week |
| Confidential client support (client_secret) | P3 | 3 days |
| Concurrent OAuth flows (queue + cancel) | P3 | 3 days |
| Vendor-side branding in callback HTML | P4 | 1 day |
| Auto-reauth on 401 from MCP server | P4 | 3 days |

---

## 7. Troubleshooting

### "unknown vendor <slug>"

The `vendor_slug` passed to `install_mcp_oauth_loopback` doesn't match any
entry in `featured_vendors()`. Check spelling (case-sensitive) or add the
vendor to the catalog first.

### "vendor <slug> is not OAuth-capable"

The vendor exists but its `install_kind` isn't `OAuthRemote`. Only vendors
with `FeaturedInstallKind::OAuthRemote { token_url, client_id_env, … }` can
use this flow.

### "token exchange failed (400 Bad Request)"

Usually one of:
- `client_id` mismatch — env var not set or wrong value
- `redirect_uri` mismatch — Shannon's `http://127.0.0.1:PORT/callback` must
  match what the vendor has registered *exactly* (including port for vendors
  that don't allow loopback wildcards)
- `code_verifier` mismatch — should never happen unless PKCE state was
  shared across requests (it isn't — `PkceContext::new()` is called per flow)

### "loopback bind failed: Address already in use"

A previous Shannon process didn't clean up. Kill any `shannon-desktop`
processes and retry. The OS will assign a fresh ephemeral port.

### Browser doesn't open

The flow uses `tauri-plugin-shell` to open the system browser. If the shell
plugin is disabled (Tauri config), the browser won't auto-open. As a
workaround, copy the authorize URL from the Shannon logs to the browser
manually.

---

## 8. Related

- [`docs/updater-setup.md`](../updater-setup.md) — Auto-updater activation
- [`docs/supply-chain.md`](../supply-chain.md) — Dependency trust model
- PR #8 (`s2/p1.4-oauth-loopback`): Tauri command + PKCE + token exchange
- PR #15 (`s2/p1.4-oauth-loopback-ui`): Featured tab UI integration
- RFC 7636: https://datatracker.ietf.org/doc/html/rfc7636
- RFC 6749 §3.1.2.4: https://datatracker.ietf.org/doc/html/rfc6749#section-3.1.2.4
