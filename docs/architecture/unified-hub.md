# ADR: Unified Extensions Hub

**Status**: Proposed (2026-06) — awaiting user approval before implementation.
**Supersedes**: `plugin-mcp.md` (2026-06) — that doc covered MCP only; this one
expands scope to skills, agents, and native data sources under a single hub.
The MCP security findings in `plugin-mcp.md` still apply and are incorporated
by reference.
**Context**: PM roadmap G1/G2 + the user's direction that "data sources /
skills / agents should be a more systematic solution — users browse and
install freely, and Shannon's developers abstract away the install
complexity."

## The problem in one paragraph

A Shannon user wants Gmail, Notion, Obsidian, a Slack summarizer skill, a
code-reviewer agent, and a Cursor-style "always write tests first" preset.
Today these come from **five different sources** with **four different
install mechanics** (OAuth, filesystem path, `.mcpb` upload, manual git
clone). Each source has its own catalog format, its own licensing, its own
trust posture. Asking the user to learn all of this is hostile. Shannon's
job is to be the universal front-end: **one hub, one Install button,
type-aware install adapter underneath**.

## Research summary

Three deep-research passes (reports under `docs/research/`):

1. **Competitor hub survey** — Claude Desktop, ChatGPT/Codex, Cursor,
   Raycast, Alfred, VS Code, JetBrains, Hermes Desktop, n8n, Zapier,
   Obsidian. Raycast Store is the clearest example of a unified hub over
   heterogeneous types.
2. **GitHub collection repo survey** — three reports covering MCP servers,
   Skills, and Agents (see References).

**Five findings drove the design:**

1. **The MCP Registry exists and is open.** `registry.modelcontextprotocol.io`
   launched Sep 2025, API-frozen at v0.1 since Oct 2025, MIT-adjacent,
   designed explicitly for programmatic client consumption. This is the
   primary seed for MCP servers — Shannon does not need to curate.
2. **SKILL.md is the de facto format for skills.** Anthropic ships
   `anthropics/skills` (149k★, Apache-2.0 examples). Codex and Copilot
   deliberately accept the same `SKILL.md` files. The format is uniform;
   distribution is not.
3. **`.mcpb` is the one-click install format for MCP.** ZIP +
   `manifest.json`, like `.vsix` for VS Code. Claude Desktop, Claude Code,
   and MCP for Windows all consume it. Spec at `modelcontextprotocol/mcpb`.
4. **`.claude-plugin/marketplace.json` is the cross-vendor plugin
   manifest.** Codex accepts it. Claude Code accepts it. Community
   marketplaces ship it (e.g. `VoltAgent/awesome-claude-code-subagents`,
   `rohitg00/awesome-claude-code-toolkit`, `anthropics/skills`).
5. **No canonical registry exists for skills or agents.** The ecosystem
   is fragmented across 9+ community awesome-lists and thousands of
   single-author repos. **Shannon must federate, not curate.**

Full details in the research reports. The rest of this ADR is the decision.

## Decision — one hub, four categories, type-aware installers

A single `/extensions` route in Shannon Desktop. Four top-level categories
in the left rail:

| Category | What lives here | Source catalog | Install mechanism |
|---|---|---|---|
| **MCP Servers** | Remote OAuth endpoints + `.mcpb` bundles + stdio escape hatch | MCP Registry (primary) + hand-curated vendor featured list | OAuth flow, `.mcpb` extract, or `add_mcp_server` config write |
| **Skills** | SKILL.md files, packaged as Claude plugins or loose files | Federated: `anthropics/skills`, `ComposioHQ/awesome-claude-skills`, `obra/superpowers`, `sickn33/antigravity-awesome-skills` | `git clone` into `~/.shannon/skills/` or marketplace install |
| **Agents** | Claude Code `.claude/agents/*.md` (subagent definitions) | Federated: `VoltAgent/awesome-claude-code-subagents`, `rohitg00/awesome-claude-code-toolkit` | Copy into `~/.shannon/agents/` (Shannon already loads this format) |
| **Data Sources** | Tier-1 native Rust integrations (Obsidian vault, Email IMAP/SMTP) | None — Shannon-built | Native: user picks a vault directory or enters IMAP credentials |

Plus a **Plugins** cross-cut section that lists entries from any category
bundled in a `.claude-plugin/marketplace.json` (these are "install five
things at once" bundles).

### The `AddonInstaller` trait

Each category has its own installer. The hub UI never branches on type —
it calls `install(entry)` and the right adapter runs.

```rust
// crates/shannon-extensions/src/installer.rs (proposed)

#[async_trait]
pub trait AddonInstaller: Send + Sync {
    fn kind(&self) -> AddonKind;          // Mcp | Skill | Agent | DataSource | Plugin
    fn supports(&self, entry: &CatalogEntry) -> bool;
    async fn install(
        &self,
        entry: &CatalogEntry,
        target: &InstallTarget,
        progress: &ProgressSink,
    ) -> Result<InstalledAddon, InstallError>;
    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError>;
    async fn update(&self, addon_id: &str) -> Result<InstalledAddon, InstallError>;
    fn requires_confirmation(&self, entry: &CatalogEntry) -> ConfirmationLevel;
}
```

Six installer implementations ship:

| Installer | Handles | Mechanism |
|---|---|---|
| `McpRegistryInstaller` | MCP servers from the open registry | Resolves to one of the four MCP installers below based on entry shape |
| `OAuthRemoteMcpInstaller` | Vendor-hosted remote MCP (Notion, Linear, Slack, GitHub, Gmail OAuth) | OAuth 2.1 PKCE → keychain token → `add_mcp_server` with URL |
| `McpbInstaller` | `.mcpb` ZIP bundles | Download → signature check → extract → register |
| `StdioMcpInstaller` | Tier-3 escape hatch (npx/uvx/docker/node/python only) | Config write to `~/.shannon/settings.json` with command allowlist |
| `MarketplacePluginInstaller` | `.claude-plugin/marketplace.json` repos | `git clone` → parse manifest → install each bundled skill/agent/MCP config |
| `NativeRustInstaller` | Tier-1 data sources (Obsidian, Email) | Enable the in-process Rust tool, store user-granted path/credentials in keychain |

The hub UI routes `entry.kind` to the matching installer. **The user never
sees these names** — they see "Connect Notion", "Install skill", "Add
agent".

### Federated catalog sources

The hub's catalog is a **materialized view** of multiple upstreams,
refreshed daily, stored in local SQLite.

**MCP Servers** (one upstream, plus featured):
- `GET https://registry.modelcontextprotocol.io/v0/servers` — paginated,
  cached, refreshed nightly. Defensive client behind a trait so v1 doesn't
  break us.
- Hand-curated `featured_vendors.json` baked into Shannon — top 15-20
  vendors with logos, OAuth hints, and verified endpoints. This is the
  "Store front page" view.

**Skills** (four federated upstreams):
- `anthropics/skills` — Apache-2.0 examples only (skip the source-available
  document skills — they need a separate license review).
- `ComposioHQ/awesome-claude-skills` — ~1000 bundled SKILL.md files,
  Apache-2.0.
- `obra/superpowers` — MIT, 20+ high-quality workflow skills.
- `sickn33/antigravity-awesome-skills` — volume play, 1500+ skills. Per-skill
  license surfaced in the UI; install gated on permissive license.

**Agents** (two federated upstreams):
- `VoltAgent/awesome-claude-code-subagents` — 100+ agents in Claude Code
  format, one MD per agent. Shannon's `custom_agent.rs` already parses
  this.
- `rohitg00/awesome-claude-code-toolkit` — 135 agents, marketplace-aware.

**Data Sources**: no upstream catalog. Shannon-curated list of native Rust
integrations (Obsidian + Email on day one; Calendar, Contacts later).

**Plugins** (cross-cut): any repo with `.claude-plugin/marketplace.json`.
The hub scans the four skill/agent upstreams for plugin manifests and lists
them in the Plugins section as "install 5 skills at once" bundles.

### Deduplication and trust signals

A skill like "deploy to vercel" exists in 12 community repos with slightly
different SKILL.md bodies. The catalog dedupes by
`(normalized_name, content_hash)` and shows the highest-trust variant as
canonical, with alternates accessible.

**Trust signals** surfaced per entry (not invented by us, read from
upstream):
- ⭐ GitHub stars (only as a weak signal)
- 📅 Last commit date
- ⚖️ License (SPDX identifier; "source-available" flagged)
- ✓ Verified by Shannon (only for the curated featured list)
- 🔒 Security scan result (if upstream provides — `openagentskill.com` and
  SkillzWave both publish scores; we link, don't recompute)

No Shannon-run quality scoring in v1. That's curation, and curation is a
maintenance cost we don't need to pay yet.

### Hub UI structure

```
/extensions
├── [Featured]      ← curated vendor cards + popular skill/agent picks
├── [MCP Servers]   ← MCP Registry mirror + featured vendors
├── [Skills]        ← federated skill catalog
├── [Agents]        ← federated agent catalog
├── [Data Sources]  ← Obsidian, Email (native)
├── [Plugins]       ← bundle installs
└── [Installed]     ← everything currently enabled, with uninstall
```

Each row in a category list: icon, name, author, short description, trust
badge, "Install" / "Connect" / "Enable" button (label depends on type).
Click opens a detail drawer with README, license, trust details, install
confirmation (for entries that require it).

## Security posture

Incorporates `plugin-mcp.md`'s MCP security section verbatim. Additions
for skills and agents:

| Attack class | Mitigation |
|---|---|
| **Malicious skill body** (prompt injection in SKILL.md instructions) | Skill bodies are shown in a "review before enabling" drawer for entries from sources without Shannon verification. After install, skills run with their `allowed-tools` field enforced. |
| **Malicious bundled script** (skill `scripts/*.sh` with `rm -rf`) | Skill scripts run through Shannon's existing bash tool permission pipeline — same prompts, same deny rules, same sandbox. No automatic execution. |
| **Agent with `tools: ['bash']` and a hostile system prompt** | Agent install shows the full system prompt and tool list in the review drawer. After install, the agent's bash calls go through the same permission classifier as the main session. |
| **Cursor rule imported as profile** (not an attack, a confusion risk) | Cursor `.cursorrules` / `.mdc` files are categorized as **Profiles**, not Agents, in the Shannon hub. Clear labeling; no false advertising. |
| **`.mcpb` with unsigned bundle** | Unsigned `.mcpb` installs require typing the binary name to confirm. Verified publisher certificates ship in Shannon's trust store; matching bundles skip the warning. |
| **Stolen OAuth token (MCP vendor)** | Tokens in OS keychain via `keyring-rs`. Per-integration revocation in the Installed tab. |

## What to avoid

- **Don't bundle any MCP server runtime in the installer.** Same as
  `plugin-mcp.md`. Users install Node themselves if they want stdio MCP.
- **Don't build a Shannon-hosted catalog API.** The MCP Registry already
  exists; use it. Community skill/agent catalogs are federated directly
  from GitHub at install time (clone-on-first-browse, refresh nightly).
- **Don't auto-install on first use.** If the user says "email Alice" and
  Email isn't configured, Shannon says so and deep-links to `/extensions`.
  Same rule as before.
- **Don't invent a Shannon-specific skill or agent format.** SKILL.md is
  the standard. Claude Code `.claude/agents/*.md` is the standard.
  Shannon-specific extensions live in optional frontmatter fields —
  document them but don't require them.
- **Don't show Cursor rules as agents.** They're prompt fragments. Show
  them in a future Profiles section, or don't show them at all in v1.
- **Don't run a Shannon-curated "featured skills" list beyond ~20 entries.**
  Curation doesn't scale. Featured = vendor MCP + 20 hand-picked skill/agent
  entries. Beyond that, search and filter.
- **Don't add a fifth installer type without an architectural review.**
  The `AddonInstaller` trait has six implementations on day one; each new
  one is a maintenance commitment.

## Trade-offs considered and rejected

| Option | Why rejected |
|---|---|
| Build Shannon's own curated catalog end-to-end (Anthropic-style) | Maintenance cost. Anthropic has a team; we don't. Federation gets 90% of the value at 10% of the cost. |
| Wait for the MCP Registry v1 GA before shipping | v0.1 is API-frozen; defensive client behind a trait is enough. Waiting costs us the Q3 window. |
| Treat Cursor `.cursorrules` as a first-class agent source | Wrong semantics. Cursor rules are system-prompt fragments, not delegatable workers. Confuses users. Defer to a future Profiles section. |
| Codex plugin format adapter on day one | Codex deliberately accepts `.claude-plugin/marketplace.json` — so our Claude Code adapter already handles most Codex plugins. A separate `.codex-plugin/plugin.json` adapter is P2 work, not P1. |
| Run all MCP stdio servers in a sandbox by default | Significant complexity (bubblewrap/firejail/seatbelt on three OSes) for a benefit that OAuth-first Tier 2 already covers. Defer. |
| Bundle a curated "top 50 skills" snapshot in the installer | Defeats the pluggable constraint. The installer ships zero skills; the hub fetches them on demand. |
| Competing agent loader unification (merge `agent_defs.rs` + `custom_agent.rs`) as part of this work | Yes it's tech debt, but it's a separate refactor. This ADR assumes `custom_agent.rs` (the Claude-compat surface) is the one the hub calls. The unification happens in a follow-up. |

## Implementation phasing

Each phase is independently mergeable. Cut after any phase.

### P1 — Catalog schema + hub shell (1-2 days)
- `shannon-extensions` crate with `CatalogEntry`, `AddonInstaller` trait,
  `InstalledAddon` types.
- `/extensions` route with empty category rails. "Coming soon" placeholders.
- SQLite mirror schema. No fetcher yet.
- Installed tab reads existing `~/.shannon/agents/`, `~/.shannon/skills/`,
  `~/.shannon/settings.json` MCP servers — shows what's already configured
  locally.

### P2 — MCP: registry ingestion + OAuth + `.mcpb` (4-5 days)
- MCP Registry fetcher (nightly refresh, defensive v0.1 client).
- `OAuthRemoteMcpInstaller` — works against Notion as canonical test.
- `McpbInstaller` — ZIP extract, manifest parse, signature verify (warn-only
  on unsigned for v1).
- Curated `featured_vendors.json` with 5 vendors: Notion, Linear, Slack,
  GitHub, Gmail-OAuth.
- Wire installed MCP entries through existing `McpProcessPool`.

### P3 — Skills: federated catalog + marketplace install (3-4 days)
- Skill catalog fetcher for the four federated upstreams.
- `MarketplacePluginInstaller` — `git clone` → parse
  `.claude-plugin/marketplace.json` → install bundled skills into
  `~/.shannon/skills/<plugin>/<skill>/`.
- Dedup by `(name, content_hash)`, trust signal surfacing.
- Review drawer for entries from non-Anthropic sources.

### P4 — Agents: catalog + marketplace install (2-3 days)
- Agent catalog fetcher for VoltAgent + rohitg00.
- Reuse `MarketplacePluginInstaller` (agents and skills share the plugin
  format).
- Install path: `~/.shannon/agents/<plugin>/<agent>.md`. Shannon's
  `custom_agent.rs` already loads this format — no parser changes.

### P5 — Native Rust data sources (3-4 days)
- Obsidian: vault picker (`tauri-plugin-dialog`) → grants FS tool access
  to the chosen path.
- Email: IMAP/SMTP credentials form → `imap` + `lettre` crates →
  keychain-stored app-specific password.
- `NativeRustInstaller` enables the in-process tool. No subprocess, no
  CVE surface (per `plugin-mcp.md` Tier 1).

### P6 — Security hardening (2-3 days)
- Tool-result prompt-injection inspection in `LlmPermissionClassifier`
  for all MCP tool results.
- `.mcpb` signature verification (warn-only → enforced for unsigned
  entries from non-featured sources).
- Per-addon revocation UI in the Installed tab.
- Skill/agent review drawer polish: full system prompt + tool list
  shown before enable.

**Total: ~3-4 weeks for all phases.** Phases P1-P2 alone give a usable
hub with MCP Registry + 5 featured vendors — that's the meaningful
milestone for the user to test the UX.

## Open questions

1. **MCP Registry API stability**: v0.1 is frozen, v1 is unscheduled. Do
   we ship the hub behind a feature flag until v1, or ship by default
   with defensive client + fallback to featured-only? Recommendation:
   ship by default, defensive client.
2. **Federated refresh cadence**: nightly refresh is the default. Do we
   also refresh on-demand when the user opens the hub (with cache TTL)?
   Recommendation: yes, with a 1-hour TTL to avoid hammering upstreams.
3. **Cursor rules / `.mdc` files**: defer to a future Profiles section,
   or skip entirely? Recommendation: defer. The Profiles concept already
   exists in Shannon (`CustomProfileRegistry`); a separate `/profiles`
   hub is the right home.
4. **Codex plugin format (`.codex-plugin/plugin.json`)**: most Codex
   plugins ship `.claude-plugin/marketplace.json` too (cross-compat).
   Do we explicitly parse `.codex-plugin/` for entries that don't, or
   accept the lossy ingestion? Recommendation: accept lossy for v1,
   add Codex adapter in P2 if users ask.
5. **Featured vendor list ownership**: who maintains `featured_vendors.json`
   in the Shannon repo? Recommendation: PR-based, gated on the vendor
   being officially recognized (verified OAuth endpoint, published
   `.mcpb`, active maintenance).
6. **Offline behavior**: if the user has no internet when they open the
   hub, do we show the cached catalog or an empty state? Recommendation:
   show cached with a "last refreshed X ago" banner.
7. **Paid-tier gating**: are any of these extensions paid-tier only? Same
   open question as `plugin-mcp.md`. Recommendation: free for all in v1,
   revisit if billing ever lands.

## Migration

No migration needed — greenfield. Existing user configs (`~/.shannon/agents/`,
`~/.shannon/skills/`, `~/.shannon/settings.json` MCP servers) are read by
the Installed tab on first launch.

The `add_mcp_server` / `remove_mcp_server` Tauri commands continue to work
for power users; the hub's installers call them under the hood for MCP
entries.

## References

- `docs/research/2026-06-mcp-integrations.md` — competitor + ecosystem
  research that drove `plugin-mcp.md`
- `docs/research/2026-06-extensions-hub-competitors.md` — competitor hub
  survey (Raycast, Alfred, Codex, n8n, Zapier, etc.)
- `docs/research/2026-06-extensions-hub-github-repos.md` — three-part
  GitHub repo inventory (MCP servers, Skills, Agents)
- `docs/architecture/plugin-mcp.md` — prior MCP-only ADR; security
  findings incorporated by reference
- `crates/shannon-mcp/` — MCP runtime (4 transports, OAuth PKCE, process
  pool, webhooks)
- `crates/shannon-skills/` — skill loader (format-superset of
  agentskills.io spec; missing marketplace layer)
- `crates/shannon-agents/src/custom_agent.rs` — Claude Code-compat agent
  loader (the surface the hub will call)
- External:
  - `https://registry.modelcontextprotocol.io` — MCP Registry API
  - `https://github.com/modelcontextprotocol/mcpb` — `.mcpb` spec
  - `https://agentskills.io/specification` — SKILL.md open standard
  - `https://github.com/anthropics/skills` — official skill marketplace
  - `https://github.com/VoltAgent/awesome-claude-code-subagents` — agent
    seed source
  - `https://github.com/ComposioHQ/awesome-claude-skills` — skill seed
    source
  - `https://github.com/obra/superpowers` — MIT skill bundle
