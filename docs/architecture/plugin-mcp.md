# ADR: Pluggable MCP Integrations (G1 Email + G2 Notion/Obsidian)

**Status**: Proposed (2026-06) — awaiting user approval before implementation.
**Context**: PM roadmap G1/G2 — Email and Notion/Obsidian MCP integrations.
**Constraint (user-stated)**: MCP integrations must be **pluggable**. Users
opt in per integration. The base installer cannot ship them bundled — we
cannot let the download grow every time we add an integration.

## Decision

Three layers, each with one job. None of the layers know about specific
integrations.

### Layer 1 — Base installer (ships with the app)

Contains the MCP runtime only: process spawning, stdio JSON-RPC framing,
tool-result normalization. **Zero integrations.** A user who never opts
into anything still gets the full AI workspace — they just don't have
email/notion/etc. tools.

### Layer 2 — Integration catalog (fetched on demand)

A JSON manifest hosted at `https://shannon.ai/integrations/index.json`. Each
entry has:

```json
{
  "id": "gmail",
  "name": "Gmail",
  "category": "email",
  "description": "Read, send, and search Gmail.",
  "runtime": "node",
  "package": "@shannon/mcp-gmail",
  "version": "1.2.3",
  "size_bytes": 4_800_000,
  "permissions": ["network:googleapis.com", "credentials:oauth"],
  "auth": "oauth-google",
  "screenshot": "...",
  "homepage": "..."
}
```

The Integrations page (`/integrations`, **not** the base Settings page)
fetches this catalog. The user sees a list with install buttons and sizes.
Nothing is installed by default.

### Layer 3 — Per-user install dir

Installed integrations land in `~/.shannon/integrations/<id>/`. This dir is
**outside** the app bundle, so:

- App updates don't touch installed integrations.
- Uninstalling the app leaves user data intact (or a cleanup script can
  remove just the integrations dir).
- Each integration is a separate npm package, isolated from the others.

## Why pluggable, not bundled

The user's constraint is correct for three reasons:

1. **Installer bloat compounds.** Bundling Email (5 MB) + Notion (8 MB) +
   Obsidian (3 MB) + GitHub (6 MB) + Linear (4 MB) = 26 MB on a 60 MB
   app = 43% growth before we ship anything new. After 10 integrations
   the installer is bigger than Slack.
2. **Dependency attack surface.** Every bundled MCP server ships its own
   transitive deps. A vuln in `imapflow@1.2.0` (used only by Email)
   becomes a CVE in Shannon Core. Opt-in means the user only takes on
   risk for integrations they actually use.
3. **License drift.** Email MCP might be MIT, Notion MCP might be
   Apache-2.0, some future integration might be GPL. Bundling forces us
   to track all of them. Opt-in keeps the base installer under one
   license.

## The three integrations in scope

### G1 — Email (read + send)

**Audience**: Users who live in their inbox. "Draft a reply to this
thread" or "summarize unread since yesterday" should just work.

**Suppliers** (pick one at install time):
- **Gmail** via Google APIs (oauth-google). Most common.
- **IMAP/SMTP** generic. For Outlook, Fastmail, self-hosted. Requires
  app-specific password.
- **Sendgrid/Postmark** outbound-only. For users who want Shannon to
  send but read elsewhere.

**Tools exposed**: `email.search`, `email.read`, `email.send`,
`email.reply`, `email.mark_read`. Five tools, no destructive ops
(`email.delete` is intentionally excluded — destructive email ops belong
in the real client).

**Auth**: OAuth for Gmail. App-specific password for IMAP. Stored in the
OS keychain (not on disk).

### G2 — Notion (read + write)

**Audience**: Users who run their life/team on Notion. "Add this to my
Weekly Notes" or "find the PRD for project X" should just work.

**Tools exposed**: `notion.search`, `notion.read_page`, `notion.create_page`,
`notion.update_block`, `notion.append_block`. Five tools.

**Auth**: Notion OAuth (notion.com/integrations).

**Scope**: User picks which databases/pages to expose at install time.
Don't request access to the entire workspace by default.

### G2b — Obsidian (read + write)

**Audience**: Users who keep their notes local. Different supplier
pattern from Notion — Obsidian is file-based, no API.

**Pattern**: Shannon reads/writes directly in the user's vault directory
(no separate MCP server needed). The "integration" is really just a
config UI that asks "where is your vault?" and a tool module that reads
`*.md` files.

**Tools exposed**: `obsidian.search`, `obsidian.read_note`,
`obsidian.create_note`, `obsidian.update_note`, `obsidian.list_backlinks`.

**Auth**: None — it's the filesystem. Permission prompt: "Allow Shannon
to read and write files in `/path/to/vault`?" → user confirms in the
standard file-permission dialog.

## What to avoid

- **Don't bundle any of these in the base installer.** Even "tiny" ones.
  The pattern breaks the moment we make an exception.
- **Don't auto-install on first use.** If the user asks Shannon to "email
  Alice" and Email isn't installed, Shannon says so and links to the
  Integrations page. Silent install crosses the opt-in line.
- **Don't share credentials across integrations.** Each integration has
  its own keychain slot. Gmail's OAuth token can't leak into Notion.
- **Don't ship `email.delete` or `notion.delete_page`.** Destructive ops
  on external systems are out of scope. User does those in the real
  client.

## Trade-offs we considered and rejected

| Option | Why rejected |
|--------|-------------|
| Bundle top 3 integrations in installer | Sets a precedent that defeats the pluggable model; 26 MB+ on day one |
| Auto-install on first tool-call use | Breaks opt-in; user has no chance to review permissions before install |
| Run all MCP servers in-process (no subprocess) | Loses the crash isolation MCP gives us; one bad integration takes down the app |
| Build our own email/notion clients (no MCP) | Locks us into maintaining those clients forever; MCP servers already exist and update independently |

## Implementation phasing

If approved, ship in this order. Each phase is independently mergeable:

1. **Catalog + Integrations page shell.** Empty page at `/integrations`
   that fetches the catalog JSON, lists entries with install buttons.
   No actual install logic yet — buttons are disabled. 1-2 days.
2. **Installer runtime.** The `~/.shannon/integrations/<id>/` layout,
   npm-pack download, signature verification, MCP-spawn. 3-4 days.
3. **G1 Email — Gmail OAuth path first.** Smallest user base, simplest
   auth. 2-3 days.
4. **G1 Email — IMAP path.** 1-2 days (protocol is the hard part; auth
   is just a password).
5. **G2 Notion.** Similar shape to Gmail OAuth. 2-3 days.
6. **G2b Obsidian.** Different shape (filesystem, not API). 2 days.

Total: ~2-3 weeks if all six phases land. Can be cut after any phase —
phase 1 alone gives us a usable Integrations page even with zero
integrations available.

## Open questions for the user

1. **Catalog hosting**: do we host `shannon.ai/integrations/index.json`
   ourselves, or use a static GitHub repo with a CDN? (Self-host = more
   control; GitHub = free, community PRs.)
2. **First integration to ship**: G1 Gmail, G1 IMAP, G2 Notion, or G2b
   Obsidian? The phasing above assumes Gmail first because OAuth is
   well-understood, but if the user's primary audience is Obsidian
   users we should ship that first.
3. **Integration sandboxing**: do we run installed MCP servers with
   reduced filesystem/network permissions (e.g. via seccomp on Linux),
   or trust the catalog signature? Sandboxing is safer but adds
   complexity.
4. **Paid vs free**: are integrations free for all users, or paid-tier
   only? Billing decision, not architectural, but it affects how we
   gate the Integrations page.
5. **Self-host**: should users be able to install an MCP server from a
   git URL (not just our catalog)? Power users will want this. Adds
   security review burden.

## References

- `src/mcp.rs` — current MCP runtime (subprocess + JSON-RPC over stdio)
- `src/commands.rs` `add_mcp_server` / `remove_mcp_server` — manual MCP
  registration that exists today; the Integrations page would call
  these after downloading the package
- `ui/src/pages/Settings.tsx` — current MCP servers UI; Integrations
  page will be a separate route (`/integrations`), not a Settings tab,
  because the install flow is too heavy for inline settings
