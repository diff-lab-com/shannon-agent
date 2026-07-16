# Deep Research: MCP Integration Landscape (2026-06)

**Date**: 2026-06-15
**Confidence**: 0.82 (high)
**Purpose**: Evidence base for `docs/architecture/plugin-mcp.md`. Drives
the G1/G2 architecture decision.

This is the full research report produced by the deep-research agent.
The ADR above cites this document; this document cites primary sources.

## Executive summary

MCP (Model Context Protocol) has decisively won the integration-protocol
war for AI desktop apps as of mid-2026. As of April 2026, **10 major AI
agents support remote MCP servers with native OAuth 2.1** (Claude
Desktop, Claude Code, ChatGPT, VS Code/Copilot, Zed, Kiro, Amazon Q,
OpenCode, Docker MCP Toolkit, Cursor), with **97M+ monthly SDK
downloads** and **10,000+ public MCP servers**. However, MCP's first 18
months have produced a **wave of critical CVEs** — the OX Security April
2026 "Mother of All AI Supply Chains" advisory documented systemic RCE
across the ecosystem (150M+ downloads affected), and every major vendor
has shipped bundled-not-installed integrations despite the open
protocol.

**Bottom line for Shannon**: Adopt MCP. Use vendor-hosted remote OAuth
servers for the catalog (Notion, Linear, Slack, GitHub). Use native
Rust tools for Obsidian (filesystem) and email (IMAP/SMTP). Expose an
Integrations page for opt-in adoption. Harden against prompt injection
and stdio command injection based on the documented CVEs. Do not bundle
MCP server runtimes in the Shannon installer.

## Competitor matrix

| Product | Integration Approach | Bundled vs Installed | Protocol | Auth | Notable Integrations |
|---------|---------------------|---------------------|----------|------|---------------------|
| **Claude Desktop** | MCP-first (Connectors UI) + Desktop Extensions (`.mcpb`) | Mostly user-installed via directory; ships built-in Node.js runtime for `.mcpb` files | MCP (stdio + Streamable HTTP + SSE) | OAuth 2.1 (remote); API key/none (local stdio); secrets in OS keychain | 50+ curated Connectors (Asana, Google Drive, Notion, Slack, Linear, GitHub, Gmail, Canva, Stripe) |
| **ChatGPT Desktop** | Proprietary "Apps" directory (formerly Connectors) + Slack app + Codex | User-installed from Apps directory | Proprietary (OpenAI-mediated REST); Slack uses Slack Marketplace app | OAuth via OpenAI account | Gmail, Notion, Slack, GitHub, Linear, Asana, Figma, Canva, DoorDash, Google Drive |
| **Cursor** | MCP-first via `.cursor/mcp.json` config + cursor-plugin bundles | User-installed via config file or `/add-plugin`; official MCP directory exists but one-click install still maturing | MCP (stdio, Streamable HTTP) | OAuth for some servers (e.g. Plain); GitHub requires PAT | GitHub (official server), Asana, Atlassian, Slack, Plain, Codacy, Sonatype, Lumen |
| **Raycast** | Proprietary TypeScript extensions in Raycast sandbox (1900+ in Store) + AI Extensions | User-installed from Store | Proprietary SDK (REST under the hood); MCP server support now also available | OAuth (per-extension), tokens in Raycast account | Notion, Slack, GitHub, Jira, Linear, Obsidian, Zoom, Google Calendar, ChatGPT, Claude, Anthropic |
| **Alfred** | Alfred Workflows (scripting + connectors); Alfred 5 Power Pack | User-installed from Alfred forum/GitHub | Scripting (bash/Python/AppleScript) + REST via connector steps | App-specific (OAuth, API keys, app passwords) | Gmail, Notion, Slack (via workflows), DEVONthink (native) |
| **Hermes Desktop** (Nous Research) | Native desktop wrapper over Hermes Agent | Bundled — agent runs locally | Proprietary (Hermes Agent tool protocol); supports remote backend via WebSocket | API keys in-app vault; vault/secret manager integration | Multi-platform messaging connectors, tool integrations table |
| **VS Code (Copilot)** | MCP-first + traditional VS Code extensions | User-installed (extension marketplace + MCP config) | MCP (stdio + HTTP) + extension API | OAuth via GitHub/Microsoft | GitHub MCP, plus full extension marketplace |
| **Windsurf** | MCP via remote URL bridge | User-installed | MCP + mcp-remote bridge | OAuth limited; mcp-remote fallback | GitHub, etc. |

**Key patterns observed:**
- Every serious competitor except Alfred uses MCP as either primary or
  co-equal integration protocol.
- Anthropic, OpenAI, and vendors (Notion, Linear, Atlassian, Asana,
  Sentry, Stripe, Cloudflare) host **remote OAuth-protected MCP servers**
  — clients add a URL and complete OAuth, no subprocess.
- Claude Desktop's `.mcpb` Desktop Extensions format (launched with
  Cowork, 2025-2026) ships a built-in Node.js runtime so users
  double-click a file to install local MCP servers without dependency
  management. Secrets land in OS keychain.
- **Cursor's pattern is closest to Shannon's likely target**:
  config-file driven, MCP-protocol, no bundling, supports both local
  (Docker/npx stdio) and remote (Streamable HTTP) servers.

## MCP ecosystem readiness (per target)

| Target | Official MCP? | Community MCP? | Install Pattern | Auth | Maturity | Notes |
|--------|--------------|----------------|-----------------|------|----------|-------|
| **Gmail** | No standalone official; Google Workspace covered by community `taylorwilsdon/google_workspace_mcp` (Drive, Calendar, Gmail, Docs, Sheets, Slides) after Anthropic archived `modelcontextprotocol/servers/gdrive` | Many community: `@modelcontextprotocol/server-gmail` (archived), Pipedream, Composio, email-mcp | npx / Docker / hosted URL | OAuth (Google) / app password (IMAP) | High (community), no single official | Gmail via MCP works well through `google_workspace_mcp` or generic IMAP server |
| **Notion** | **Yes** — official `makenotion/notion-mcp-server` (4.1k stars, MIT, v2.1.0 Jan 2026, TypeScript 99.1%); also hosted remote OAuth server | Many community forks (Notion Local Operations, ClawLink, Better Notion) | `npx @notionhq/notion-mcp-server` or hosted URL via Connectors UI | OAuth 2.1 (remote) or `NOTION_TOKEN` integration key (local) | **Very High** — official, mature | Notion explicitly supports ChatGPT Pro, Claude, Cursor via official MCP |
| **Obsidian** | **No official MCP** (Obsidian has no vendor MCP server; vault is local markdown files) | **88+ community MCP servers** (PulseMCP); `coddingtonwear/obsidian-local-rest-api` plugin now ships **built-in MCP server** (Streamable HTTP, bearer token) — recommended over third-party | Plugin (in-vault) OR filesystem-access MCP server (Piotr1215/mcp-obsidian, `mcp-obsidian`) OR Local REST API plugin's built-in `/mcp/` | API key (bearer token) or none (filesystem) | High (community + Local REST API plugin's built-in) | **Recommendation: filesystem access for Shannon**, since Shannon already has file ops. No need to install Obsidian's REST API plugin unless you want structured metadata. |
| **GitHub** | **Yes** — official `github/github-mcp-server` (Go, Docker image `ghcr.io/github/github-mcp-server`, 100+ tools) | Original `modelcontextprotocol/servers/github` was archived 2025; superseded by official | Docker run OR hosted remote URL (`https://api.githubcopilot.com/mcp/`) OR `npx @modelcontextprotocol/server-github` (legacy) | PAT (primary) or OAuth on hosted | **Very High** — official, GitHub-maintained | Canonical example of vendor-hosted remote MCP; Cursor/Claude/VS Code all document first-class support |
| **Linear** | **Yes** — official hosted remote server at `https://mcp.linear.app/...` (Streamable HTTP, OAuth 2.1 with dynamic client registration) | Community wrappers via Composio/Pipedream | Add URL to MCP client, OAuth flow | OAuth 2.1 with PKCE | **High** — official, vendor-hosted | Linear is one of the cited 2026 examples of vendor-operated OAuth-protected MCP |
| **Slack** | **Yes** — official Slack MCP server released as Claude Connector on Jan 26 2026 (bidirectional: Claude can post proactively, users can @mention Claude from Slack); also available as ChatGPT Slack Marketplace app | Community: `@modelcontextprotocol/server-slack` (archived reference), Composio-hosted | Add URL via Connectors UI OR Slack Marketplace (for ChatGPT) | OAuth 2.1 (Slack workspace install) | **High** — official | Slack MCP supports proactive posting from Claude and @mention-triggered agent runs |
| **IMAP / SMTP** (generic) | No official | Many community: `mail-mcp` (Rust, 30 tools, IMAP+SMTP+EWS+Graph), `@codefuturist/email-mcp`, `ai-zerolab/mcp-email-server` (Python, TUI config), IMAP MCP (agentpedia) | npx / uvx / Docker | IMAP app password / OAuth2 (for Gmail/M365) | **Medium-High** (community, multiple options) | Best option: `tecnologicachile/mail-mcp` (Rust, multi-provider, real OAuth2) for production; `@codefuturist/email-mcp` for stdio simplicity |

**Ecosystem verdict:** For all six targets Shannon cares about, MCP
coverage is **ready today**. Official vendor-hosted servers exist for
Notion, GitHub, Linear, Slack. Gmail/Obsidian/IMAP are covered by
mature community servers. Obsidian is the one case where raw filesystem
access is simpler and arguably better than an MCP wrapper.

## Non-MCP patterns (and when they fit)

| Pattern | Used By | Auth | When it fits | When MCP is better |
|---------|---------|------|--------------|-------------------|
| **Raycast Extensions** (TypeScript, sandboxed) | Raycast | Per-extension OAuth | Launcher UX, keyboard-driven, short-lived actions, no long-running agents | When you need LLM reasoning over tool outputs across multiple tools |
| **Alfred Workflows** (scripting) | Alfred 5 Power Pack | App-specific (OAuth/API key/app password) | macOS-only power-user automation, chained scripts | Cross-platform, LLM-orchestrated workflows |
| **VS Code Extensions** | VS Code, Cursor (fork) | OAuth (publisher) | In-editor features (linters, language servers, UI panels) | When extension is the agent, not when extension feeds an agent |
| **Apple Shortcuts / Automator** | macOS / iOS | iCloud OAuth, app intents | iOS shortcuts, Siri triggers, no-code automation | Anything cross-platform or LLM-driven |
| **n8n / Zapier / Make** (SaaS automation) | Thousands of businesses | Per-app OAuth managed by platform | No-code ETL, scheduled jobs, connecting non-AI apps | Real-time agent tool calls, low-latency interactive flows |
| **Direct OAuth + REST** (no MCP) | Most legacy SaaS integrations, Gumloop, Relay.app | OAuth per provider | One-off integration, no LLM in the loop | Anything where the LLM picks tools dynamically |

**Insight for Shannon:** MCP's value is **dynamic tool discovery +
standard schema** so the LLM can pick the right tool without custom
code per integration. For a fixed integration with no LLM
orchestration, direct REST is simpler. Since Shannon IS an LLM
workspace, MCP is the right default — but a few "always-on"
integrations (Obsidian file access, local git operations) may be
better as **in-process native tools** for performance.

## Security findings

This is the most important section for an architecture decision. **MCP
has had a severe vulnerability year.**

### Documented CVEs (selected, all 2025-2026)

| CVE | Product | Severity | Issue |
|-----|---------|----------|-------|
| **CVE-2026-30615** | Windsurf 1.9544.26 | **Critical (zero-click)** | Prompt injection via rendered HTML silently modified local MCP config and registered malicious stdio server — no user interaction required |
| **CVE-2026-30617** | LangChain-Chatchat 0.3.1 | Critical | Unauthenticated remote MCP stdio config → RCE |
| **CVE-2026-30624** | Agent Zero 0.9.8 | Critical | External MCP server JSON config executes arbitrary commands |
| **CVE-2026-30623** | Anthropic MCP SDK (stdio transport) | Critical | `StdioServerParameters` executes whatever `command` it's handed — **affects Python/TypeScript/Java/Rust SDKs across 150M+ downloads, 7,000+ servers, up to 200,000 vulnerable instances**. Anthropic confirmed as intentional design choice, declined to change protocol |
| **CVE-2026-26029** | sf-mcp-server (Salesforce) | Critical | `child_process.exec` command injection |
| **CVE-2026-0755** | gemini-mcp-tool | Critical (CVSS 9.8) | Unsanitized input to `execAsync` — **zero-day, no patch at disclosure** |
| **CVE-2025-68143/44/45** | Anthropic's own `mcp-server-git` | Critical (chained) | Path validation bypass + unrestricted `git_init` (can turn `.ssh` into a git repo) + argument injection in `git_diff` → full RCE via malicious `.git/config` |
| **CVE-2026-22785** | Orval MCP client codegen | High | OpenAPI spec `summary` field injected as code |
| **CVE-2026-30615 (Cursor, Claude Code)** | Cursor / Claude Code | Critical | Required some user interaction but attack chain viable |

**Key attack patterns documented:**
1. **Indirect prompt injection** → tool result content embeds malicious
   instructions → LLM calls vulnerable tool with attacker payload (Figma
   comments, GitHub READMEs, web pages).
2. **DNS rebinding** → victim visits malicious website → routed to
   their local MCP server → command execution without any AI
   interaction.
3. **Tool poisoning** → malicious tool descriptions in remote servers
   manipulate the LLM into calling attacker-chosen tools.
4. **stdio command injection** → MCP client passes user-controlled
   `command` field to subprocess without validation (this is the
   design-level flaw Anthropic declined to fix).

**The "Client as WAF" insight:** Claude Desktop and Gemini Advanced
often refuse to exploit vulnerable servers because of their own safety
layers. Researchers warn this is a false defense — relying on the
client to refuse is not a server-side fix. Shannon should treat this as
motivation to **harden Shannon itself**, not assume server-side safety.

**Implication for Shannon Desktop:**
- **Whitelist stdio `command` basenames** (as LiteLLM did in their
  fix) — never pass arbitrary `command` to subprocess.
- **Treat MCP tool results as untrusted content** — render with
  prompt-injection-aware sanitization; consider isolation markers
  around tool outputs (Shannon already has structure for this via
  `ToolProgress`).
- **Prompt-injection-aware permission classifier** — Shannon already
  has `LlmPermissionClassifier`. Wire it to inspect tool result
  content for high-risk patterns.
- **Prefer remote OAuth MCP servers over local stdio** wherever
  possible — no subprocess, no command injection surface.
- **Sandbox local MCP servers** — if Shannon does spawn stdio servers,
  sandbox them (seccomp/AppArmor on Linux, sandbox-exec on macOS).

## Installer size data

Hard numbers are scarce and partially unverified; this is what is
documented:

| Product | Installer Size | Notes | Source quality |
|---------|----------------|-------|----------------|
| **Claude Desktop (macOS, .pkg/.dmg)** | **~295 MB** initial download; chat-only needs ~1GB disk; **Cowork VM image silently downloads ~13 GB** on every launch (cannot be disabled by individual users, even if unused) | 13 GB silent download is widely criticized; re-downloads if deleted | Verified — Reddit thread with Anthropic support confirmation |
| **Claude Desktop (Windows)** | "6-300 MB" range reported by MajorGeeks; Uptodown lists 6.68 MB (likely a stub downloader) | Windows ships as MSIX; some corporate machines block MSIX by policy | Verified — MajorGeeks, Verified — Uptodown |
| **Cursor** | **No clean published number** [unverified] — install reports mention ".pack files" growing unbounded, users report 500MB-2GB+ working set; built on VS Code Electron base so similar footprint to VS Code (~300-400MB installer) | Snapshot system has no disk-usage limit, .pack files accumulate | Forum thread, [unverified installer size] |
| **Raycast** | **~80-100 MB** [unverified] — Electron-free (native Swift), substantially smaller than Electron apps | Native macOS app, no bundled Node runtime | [unverified — based on Raycast native architecture] |
| **Integrations share of installer** | **Not separately reported** [unverified] — Claude Desktop's built-in Node.js runtime (for `.mcpb` Desktop Extensions) is the most significant "integration tax", reportedly 50-100MB | No vendor publishes a breakdown | [unverified] |

**Key insight:** The 13GB Claude Cowork VM download is the extreme
outlier — Shannon should NOT do this. Claude Desktop's built-in Node
runtime for `.mcpb` is the more relevant comparison: it adds ~50-100MB
to enable one-click local MCP server install. Shannon could avoid this
by **requiring users to have Node installed** OR by **only supporting
remote OAuth MCP servers** (no local stdio servers in v1).

## Recommended approach for Shannon Desktop

See `docs/architecture/plugin-mcp.md` § Decision for the full
recommendation. Summary:

- **Tier 1 (native Rust, no MCP)**: Obsidian via filesystem tool,
  Email via IMAP/SMTP using `imap` + `lettre` crates.
- **Tier 2 (vendor-hosted remote MCP, OAuth)**: Notion, Linear, Slack,
  GitHub. Shannon adds an Integrations UI, does OAuth, stores tokens
  in OS keychain.
- **Tier 3 (escape hatch)**: Custom URL field for remote MCP servers.
  Local stdio allowed but with strict command allowlist.

## Open questions unresolved by research

1. **Cursor installer size** — could not find a verified published
   number. The 300-400MB estimate is based on its VS Code Electron
   lineage, not a primary source. [unverified]
2. **Raycast installer size** — no verified number found; 80-100MB
   estimate based on native Swift architecture, not direct measurement.
   [unverified]
3. **Anthropic Cowork's 13GB silent download** — confirmed real and
   widely criticized, but unclear if it's still present in the very
   latest version (June 2026). The Reddit thread is from a recent but
   undated build.
4. **User preference research (bundled vs opt-in)** — no academic or
   industry user-research study was found that directly compares these
   approaches. The signal is indirect: every successful competitor has
   moved to opt-in via directory, suggesting market preference, but
   this is inference not data.
5. **Hermes Desktop's integration approach specifics** — exists
   (github.com/fathah/hermes-desktop, Nous Research docs) but docs are
   sparse on which specific third-party integrations are supported
   today. The "tool integrations" and "messaging platforms" sections
   of the README suggest a proprietary tool protocol, not MCP, but
   this was not fully verified.
6. **Whether Anthropic will change the stdio command design** following
   the April 2026 OX advisory — Anthropic declined as of the
   disclosure. Shannon should assume the design stays as-is and harden
   on the client side.
7. **MCP subprocess overhead in Shannon specifically** — no
   Shannon-specific benchmarks. General data: stdio is 10-20ms per op
   (10,000+ ops/sec), HTTP is 80-300ms (100-1000 ops/sec). For
   Shannon's interactive use case, stdio is fast enough; the LLM is
   always the bottleneck, not the transport.

## Sources

### Adoption & competitor analysis
- [Truthifi — State of MCP 2026](https://truthifi.com/education/state-of-mcp-2026-ai-agents-custom-connectors)
- [Claude Help — Custom Connectors using remote MCP](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp)
- [meetingnotes.com — 21 Favorite Claude Connectors 2026](https://meetingnotes.com/blog/best-claude-connectors)
- [natoma.ai — Claude Desktop MCP Setup](https://natoma.ai/blog/how-to-enabling-mcp-in-claude-desktop)
- [natoma.ai — Cursor MCP Setup](https://natoma.ai/blog/how-to-enabling-mcp-in-cursor)
- [houtini.com — Claude Desktop System Requirements](https://houtini.com/articles/claude-desktop-system-requirements)
- [Reddit — Claude Desktop 13GB silent download](https://www.reddit.com/r/ClaudeAI/comments/1rlc71n/claude_desktop_app_silently_downloads_a_13_gb)
- [chatgpt.com — Apps in ChatGPT](https://chatgpt.com/features/apps)
- [DEV Community — Chat with Apps in ChatGPT](https://dev.to/alifar/chat-with-apps-in-chatgpt-the-future-of-connected-work-1ja2)
- [Slack Marketplace — ChatGPT](https://slack.com/marketplace/A097V82EGG2-chatgpt)
- [Cursor Community Forum — MCP auth](https://forum.cursor.com/t/slack-mcp-auth-fails-with-must-use-pkce-to-redirect-to-a-non-web-uri/158746)
- [Raycast](https://www.raycast.com)
- [Raycast AI](https://www.raycast.com/core-features/ai)
- [Highlight vs Raycast](https://highlightai.com/compare/raycast)
- [GitHub — fathah/hermes-desktop](https://github.com/fathah/hermes-desktop)
- [Nous Research — Hermes Agent Desktop docs](https://hermes-agent.nousresearch.com/docs/user-guide/desktop)

### Per-integration readiness
- [GitHub — github/github-mcp-server Cursor install](https://github.com/github/github-mcp-server/blob/main/docs/installation-guides/install-cursor.md)
- [GitHub — makenotion/notion-mcp-server](https://github.com/makenotion/notion-mcp-server)
- [Linear Docs — MCP](https://linear.app/docs/mcp)
- [note.com — Era Where Claude Can Control Slack/Notion/GitHub](https://note.com/snake_dragon/n/nd46ae80e3304)
- [hidekazu-konishi.com — MCP Server Ecosystem Reference 2026](https://hidekazu-konishi.com/entry/mcp_server_ecosystem_reference_2026.html)
- [GitHub — modelcontextprotocol/servers](https://github.com/modelcontextprotocol/servers)
- [GitHub — coddingtonbear/obsidian-local-rest-api](https://github.com/coddingtonbear/obsidian-local-rest-api)
- [GitHub — Piotr1215/mcp-obsidian](https://github.com/Piotr1215/mcp-obsidian)
- [PulseMCP — Obsidian servers](https://www.pulsemcp.com/servers?q=obsidian)
- [mcpservers.org — mail-mcp](https://mcpservers.org/servers/tecnologicachile/mail-mcp)
- [GitHub — ai-zerolab/mcp-email-server](https://github.com/ai-zerolab/mcp-email-server)

### Security
- [OX Security — MCP Supply Chain Advisory](https://www.ox.security/blog/mcp-supply-chain-advisory-rce-vulnerabilities-across-the-ai-ecosystem)
- [CSA Lab Note — MCP by Design RCE](https://labs.cloudsecurityalliance.org/research/csa-research-note-mcp-by-design-rce-ox-security-20260420-csa)
- [LiteLLM — CVE-2026-30623 security update](https://docs.litellm.ai/blog/mcp-stdio-command-injection-april-2026)
- [vulnerablemcp.info](https://vulnerablemcp.info)
- [endorlabs.com — Classic Vulnerabilities Meet AI Infrastructure](https://www.endorlabs.com/learn/classic-vulnerabilities-meet-ai-infrastructure-why-mcp-needs-appsec)
- [SentinelOne — CVE-2026-26029](https://www.sentinelone.com/vulnerability-database/cve-2026-26029)
- [SentinelOne — CVE-2025-53107](https://www.sentinelone.com/vulnerability-database/cve-2025-53107)
- [penligent.ai — CVE-2026-0755 deep analysis](https://www.penligent.ai/hackinglabs/deep-analysis-of-gemini-mcp-tool-command-injection-cve-2026-0755-when-an-mcp-toolchain-hands-user-input-to-the-shell)

### Performance / protocol trade-offs
- [tyk.io — MCP vs CLI Enterprise Comparison](https://tyk.io/learning-center/mcp-vs-cli-for-ai-agents-enterprise-comparison-guide)
- [dev.to — MCP Transport Hot Take](https://dev.to/leomarsh/hot-take-most-mcp-implementations-are-choosing-the-wrong-transport-layer-5ddo)
- [kirkryan.co.uk — stdio vs Streamable HTTP](https://kirkryan.co.uk/stdio-vs-streamable-http-choosing-the-right-mcp-transport)
- [systemprompt.io — MCP vs CLI in Claude Code](https://systemprompt.io/guides/mcp-vs-cli-tools)
