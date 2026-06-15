# ADR: Pluggable Integrations (G1 Email + G2 Notion/Obsidian)

**Status**: Revised (2026-06) — supersedes the prior 2026-06 proposal.
Based on a deep-research pass over competitor integrations (Claude
Desktop, ChatGPT Desktop, Cursor, Raycast, Hermes Desktop) and the
current MCP ecosystem. Awaiting user approval before implementation.
**Context**: PM roadmap G1/G2. **Constraint (user-stated)**: integrations
must be **pluggable**, user opt-in per integration, base installer cannot
bundle them.

## Research summary (what changed my mind)

My prior proposal invented a three-layer "Shannon-hosted integration
catalog" architecture from first principles. The research killed it.
Three findings forced a rewrite:

1. **MCP won the protocol war.** As of April 2026, 10 major AI agents
   support remote MCP with native OAuth 2.1: Claude Desktop, Claude Code,
   ChatGPT, VS Code/Copilot, Zed, Kiro, Amazon Q, OpenCode, Docker MCP
   Toolkit, Cursor. 97M+ monthly SDK downloads, 10,000+ public MCP
   servers. Building a proprietary "Shannon integration protocol" would
   ignore this.
2. **Vendors host their own MCP servers.** Notion, Linear, Slack,
   GitHub, Atlassian, Asana — they all run hosted remote MCP servers
   with OAuth. Shannon does not need to write Gmail/Notion/Linear
   clients. We need an Integrations UI that wires the user to these
   existing endpoints.
3. **MCP's stdio transport has a documented critical CVE class.** The
   April 2026 OX Security "Mother of All AI Supply Chains" advisory
   showed systemic RCE: CVE-2026-30623 in Anthropic's own MCP SDK
   affected 150M+ downloads because `StdioServerParameters` executes
   whatever `command` it's handed. Anthropic declined to fix it. This
   reshapes the local-vs-remote trade-off.

Full research report is at `docs/research/2026-06-mcp-integrations.md`
(see References). Decision below is based on it.

## Decision — three tiers, none bundled

Shannon ships **zero integrations in the base installer**. Every
integration is opt-in. The architecture splits integrations into three
tiers based on what they touch and how:

### Tier 1 — Native in-process Rust tools (no MCP, no subprocess)

For integrations that are better as native code than as MCP servers:

- **Obsidian** — vault is just `*.md` files on disk. The existing
  Shannon filesystem tool already does this. We add a "Connect Obsidian
  Vault" flow that grants the tool access to a vault directory the user
  picks. No MCP, no subprocess, no new dependencies.
- **Email (Gmail IMAP, Fastmail, Outlook, self-hosted)** — implement
  IMAP + SMTP directly in `shannon-tools` using the `imap` and `lettre`
  crates. Shannon is Rust; we don't need to spawn a Node or Python MCP
  server to read mail. This **completely avoids the stdio
  command-injection CVE class** because there is no subprocess.

Why native over MCP for these two:
- Obsidian doesn't have an official MCP server (it's filesystem).
- Email MCP servers exist (`tecnologicachile/mail-mcp`, etc.) but
  spawning one gives us all the subprocess attack surface for zero
  benefit — Shannon is already a long-running Rust process.

### Tier 2 — Vendor-hosted remote MCP (OAuth 2.1, no subprocess)

For integrations whose vendors run their own MCP servers:

| Integration | Endpoint | Auth |
|-------------|----------|------|
| **Notion** | `https://mcp.notion.com/mcp` | OAuth 2.1 (Notion integration) |
| **Linear** | `https://mcp.linear.app/mcp` | OAuth 2.1 with PKCE |
| **Slack** | Slack MCP server (released Jan 2026) | OAuth 2.1 (workspace install) |
| **GitHub** | `https://api.githubcopilot.com/mcp/` | PAT or OAuth |
| **Gmail (OAuth path)** | community `taylorwilsdon/google_workspace_mcp` hosted URL | OAuth 2.1 (Google) |

Shannon's job here is the **Integrations UI**, not the integration code:

1. Show a catalog of vendor-hosted servers (one row per integration).
2. User clicks "Connect".
3. Shannon opens the OAuth flow in the system browser.
4. Token comes back, stored in the OS keychain (Tauri has `keyring-rs`).
5. Shannon's existing MCP runtime (`shannon-mcp` crate) connects to the
   server and discovers tools via `tools/list`.

No subprocess. No command injection surface. Vendor maintains the
server — Shannon maintains the OAuth plumbing.

### Tier 3 — User-installed custom MCP servers (escape hatch)

For users who want to wire up something we don't curate:

- **Remote URL field**: paste any Streamable HTTP MCP URL. Shannon
  treats it like a Tier 2 server.
- **Local stdio server** — supported but **gated behind a strict
  command allowlist**. We will not accept arbitrary `command` fields
  (lesson from CVE-2026-30623). Curated allowlist of safe invokers:
  `npx`, `uvx`, `docker`, `node`, `python`. Anything else requires the
  user to type the binary name manually and confirm a warning dialog.

## Security posture

The research documented a sustained CVE wave in MCP-adjacent code.
Shannon's mitigations:

| Attack class | Mitigation |
|--------------|------------|
| **stdio command injection** (CVE-2026-30623) | Tier 1 has no subprocess. Tier 2 has no subprocess. Tier 3 allows stdio but with command allowlist + manual override. |
| **Indirect prompt injection via tool result** | Tool result content is treated as untrusted. Shannon's existing `LlmPermissionClassifier` will inspect tool results for embedded `command:`-style instructions before re-injecting into context. |
| **Tool poisoning** (malicious tool descriptions) | Tier 2 catalog is curated (we only ship entries pointing at vendor-owned domains). Tier 3 shows tool descriptions in a review panel before activation. |
| **DNS rebinding against localhost MCP** | We do not auto-trust localhost. All Tier 3 servers require explicit user activation. |
| **Stolen OAuth token** | Tokens live in OS keychain, not on disk. Revocation flow in the Integrations UI. |

These are not nice-to-haves. They are the reason Shannon should not
spawn subprocess MCP servers casually.

## Why this approach

1. **Aligns with where the ecosystem actually is.** Every major AI
   desktop competitor except Alfred uses MCP. Building proprietary would
   re-invent a solved problem and lock Shannon out of 10,000+ servers.
2. **Vendor-hosted remote servers shift maintenance burden.** Notion's
   server breaks → Notion fixes it. We don't write or maintain Gmail /
   Notion / Linear clients.
3. **Native Rust for Obsidian + email is the right architecture.**
   Shannon is Rust. Obsidian is filesystem. Email has mature Rust
   crates. Subprocess MCP servers add zero value here and all the CVE
   risk.
4. **Honors the user's pluggable constraint.** Base installer ships no
   integrations and no MCP server runtimes. Tier 1 ships as part of
   Shannon but is inert until the user grants it a path / credentials.
   Tier 2 and 3 are pure opt-in.
5. **Builds on Shannon's existing MCP infrastructure.** `shannon-mcp`
   crate already supports `mcpServers` config in `.mcp.json`,
   `~/.claude/settings.json`, `~/.shannon/settings.json`. The work is
   UX + catalog + keychain plumbing, not new protocol implementation.

## What to avoid

- **Don't bundle any MCP server runtime in the installer.** No
  built-in Node.js for `.mcpb`-style install (Claude Desktop does this;
  it adds 50-100 MB and exposes the CVE surface). Users who want local
  stdio servers install Node themselves.
- **Don't ship a Shannon-hosted integration catalog at `shannon.ai/integrations/index.json`.**
  Use the existing PulseMCP / mcpservers.org directories instead.
  Self-hosting a catalog is maintenance cost without value.
- **Don't auto-install on first tool use.** If the user asks Shannon to
  "email Alice" and Email isn't configured, Shannon says so and links
  to `/integrations`. Silent install crosses the opt-in line.
- **Don't allow arbitrary `command` strings in stdio MCP config.**
  Allowlist only. Manual override requires typing the binary name +
  confirming a warning.
- **Don't share credentials across integrations.** Each integration
  gets its own keychain slot.
- **Don't ship destructive tools** (`email.delete`, `notion.delete_page`).
  Destructive ops on external systems belong in the real client.

## Trade-offs we considered and rejected (updated with research)

| Option | Why rejected (with research) |
|--------|------------------------------|
| Build our own integration protocol instead of MCP | Every competitor adopted MCP (Truthifi 2026 report). Proprietary would isolate Shannon. |
| Bundle top 3 integrations in the installer | Claude Desktop's 13 GB Cowork VM download is the cautionary tale (Reddit r/ClaudeAI thread, 2026). Opt-in wins. |
| Spawn community MCP servers for Obsidian and email | Obsidian has no official server and 88+ uneven community ones (PulseMCP). Email MCP servers exist but add subprocess CVE surface for no benefit when Shannon is already Rust. Native is better here. |
| Support `.mcpb` Desktop Extensions format | Requires shipping Node.js runtime (50-100 MB installer tax). Defer until users ask for it; Tier 3 stdio allowlist covers the same need. |
| Auto-install on first tool-call use | Breaks opt-in; user has no chance to review permissions. |
| Trust localhost MCP without confirmation | DNS rebinding attacks documented (endorlabs 2026 analysis). |

## Implementation phasing

Each phase is independently mergeable. Cut after any phase.

1. **Integrations page shell** (`/integrations`). Empty catalog UI,
   fetches entries from a built-in constant (no remote catalog). Tier 2
   row renders but Connect button is disabled. 1-2 days.
2. **OAuth + keychain plumbing.** Tier 2 Connect works for Notion only
   (canonical test case). Token stored via `keyring-rs`. Shannon's MCP
   runtime connects to the URL with the token. 3-4 days.
3. **Expand Tier 2 catalog**: Linear, Slack, GitHub. Each one is a
   catalog entry + maybe a per-vendor OAuth quirk. 1 day each.
4. **Obsidian Connect Vault** (Tier 1). UI to pick a vault directory,
   grant filesystem tool access to it. 1-2 days.
5. **Native IMAP/SMTP** (Tier 1). New `EmailTool` in `shannon-tools`,
   IMAP search/read + SMTP send. App-specific password storage in
   keychain. 3-4 days.
6. **Gmail OAuth** (Tier 2 via `google_workspace_mcp` hosted URL).
   1-2 days.
7. **Tier 3 escape hatch** — custom URL field with allowlisted stdio.
   2-3 days.
8. **Security hardening pass** — prompt-injection inspection in
   `LlmPermissionClassifier` for tool results; per-integration
   revocation UI; tool-description review panel for Tier 3. 2-3 days.

Total: ~3-4 weeks for all phases. Phases 1-3 alone give a usable
Integrations page with 4 vendors wired up — that is the meaningful
milestone.

## Open questions for the user

1. **Tier 1 email: IMAP-first or Gmail OAuth-first?** Research says
   both are shippable. IMAP covers Gmail/Fastmail/Outlook/self-hosted
   with one implementation. Gmail OAuth covers only Gmail but is what
   most users will want. My recommendation: ship IMAP first (broadest
   coverage, no Google Cloud project setup needed), then add Gmail
   OAuth.
2. **Tier 2 catalog: which vendors first?** Notion has the most mature
   official MCP server (`makenotion/notion-mcp-server`, 4.1k stars,
   active). Recommend Notion → Linear → Slack → GitHub order, based on
   ecosystem maturity.
3. **Tier 3 stdio allowlist**: ship with `npx`, `uvx`, `docker`, `node`,
   `python`? Or start narrower (just `npx` + `uvx`) and expand on
   request?
4. **Tier 3 manual override**: when a user wants to run a stdio MCP
   server with a binary not in the allowlist, how scary is the warning?
   (Just text? Type-the-binary-name-to-confirm? Both?)
5. **Paid vs free**: are Tier 2 integrations free for all Shannon users,
   or paid-tier only? (Same open question as before — billing decision,
   not architectural, but affects how we gate the Integrations page.)
6. **Catalog source**: hard-code the Tier 2 catalog in the Shannon
   binary (my recommendation — updates ship with app updates), or fetch
   from a remote URL at runtime (faster iteration but adds a network
   dependency)?

## Migration

No migration needed — this is greenfield. Existing `add_mcp_server` /
`remove_mcp_server` Tauri commands in `src/commands.rs` continue to work
for power users; the Integrations UI calls them under the hood for Tier
2/3 entries.

## References

- `docs/research/2026-06-mcp-integrations.md` — full deep-research
  report (competitor matrix, MCP ecosystem readiness, security findings,
  installer-size data, sources)
- `src/mcp.rs` — current MCP runtime (subprocess + JSON-RPC over stdio)
- `src/commands.rs` `add_mcp_server` / `remove_mcp_server` — existing
  manual MCP registration; Integrations UI calls these after OAuth
- `ui/src/pages/Settings.tsx` — current MCP servers UI; the Integrations
  page is a separate `/integrations` route (install flow is too heavy
  for inline settings)
- OX Security April 2026 advisory — `mother-of-all-ai-supply-chains`
  CVE-2026-30623 and related
- LiteLLM April 2026 fix — reference implementation of stdio command
  allowlisting
- Truthifi State of MCP 2026 report — competitor adoption data
