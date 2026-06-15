# ADR: Landing Page (G5)

**Status**: **Deferred** (2026-06) — wait until development is
substantially complete before revisiting. Per user direction: "请等开发
基本完成后最后考虑". Document kept as a placeholder so the design notes
survive; will be revived when the rest of the roadmap lands.
**Context**: PM roadmap G5 — marketing landing page that reflects the
Sprint 1 repositioning from "AI Code Assistant" to "Your AI Workspace".

## Decision

A single landing page at `https://shannon.ai/` that does **one job**:
explain what Shannon is, in under 30 seconds, to someone who has never
heard of it. Everything else (docs, download, blog, pricing) lives on
other routes.

The page has **five sections**, in this order:

### 1. Hero

One sentence. One sub-sentence. One CTA.

> **Shannon — Your AI Workspace**
>
> Chat, tasks, automations, and agents in one desktop app. Your work,
> not your code's.
>
> [Download for macOS / Windows / Linux]

The verb "workspace" is doing the heavy lifting. The old positioning
("AI Code Assistant") pigeonholed Shannon as a coding tool. The new
positioning ("AI Workspace") covers coding + email + notes + tasks +
anything the integrations touch.

The download button is the **only** CTA above the fold. No "Watch demo",
no "Sign up for free". The page exists to drive installs.

### 2. What it does (4 cards)

The four cards from the new sidebar (Sprint 1 brand work):

| Card | Icon | One-liner |
|------|------|-----------|
| **Chat** | chat_bubble | Talk to your AI. With context, tools, and memory. |
| **Tasks** | calendar_clock | Schedule recurring work. Cron, retries, budgets. |
| **Automation** | bolt | Hook events trigger routines. 30+ events. |
| **Agents** | smart_toy | Spawn parallel teammates. Per-agent model and tools. |

This is the only place on the page that enumerates features. No
feature-grid of 30 bullets; the cards are the menu.

### 3. Why it's different (3 contrasts)

Each row is "them vs. us". The reader already uses ChatGPT / Claude /
Cursor — we have to differentiate or we're noise.

| Them (status quo) | Shannon |
|---|---|
| Cloud-only chat in a browser tab | Native desktop app, real file paths, real bash |
| One assistant, no parallelism | Agent teams with worktree isolation |
| Manual everything | Routines + scheduled tasks fire on events |

Three rows, not seven. The reader gets the pattern after three.

### 4. Integrations (preview, not list)

A single row of integration logos (Gmail, Notion, Obsidian, GitHub,
Linear, Slack). Even if only Gmail and Notion are shipping, show the
row — it signals "this is extensible". Below: "Pluggable by design.
Install only what you use."

Cross-link to `/integrations` once that page exists.

### 5. Footer

- Download (all three OSes)
- Docs link
- GitHub link
- Privacy / Terms

No newsletter signup. No "follow us on Twitter". A landing page that
asks for more than one action gets less of any of them.

## Why a page, not a docs site

- **Docs answer "how"**. The landing page answers "why should I care".
  Conflating them means the landing page is unreadable and the docs are
  unfocused.
- **Docs are for users**. The landing page is for prospects. Different
  audience, different writer, different page.
- **Docs can be long**. The landing page is one screen at 1080p. If it
  scrolls, it's too long.

## Trade-offs we considered and rejected

| Option | Why rejected |
|--------|-------------|
| Multi-page marketing site (home + features + pricing + blog) | We don't have the marketing bandwidth to maintain four pages; one well-crafted page beats four mediocre ones |
| Hero with autoplay video background | Bandwidth-heavy, kills performance on mobile, and the value of motion here is low |
| Comparison table with named competitors | Reads as insecure; let the contrasts in section 3 do the work without naming names |
| Inline interactive demo | A demo that fits in a hero section is too shallow to be impressive; a real demo needs its own page |
| Pricing on the landing page | Pricing is currently free / paid-tier undecided (see plugin-mcp open question #4). Don't ship pricing until it's stable. |

## Tech stack

**Recommended**: Static HTML + Tailwind via CDN, hosted on Cloudflare
Pages or Vercel. No React. No Next.js. No backend.

Reasoning:
- The page has zero interactivity beyond anchor links. React would add
  100 KB of JS for nothing.
- Static HTML renders in under 100 ms on any connection.
- Updates are git-push-to-deploy. No build step means copy changes
  don't require a developer.

**Asset budget**: < 200 KB total (HTML + CSS + hero image). The
integration logos are SVGs at < 2 KB each.

## Copy decisions worth flagging

- **"Your work, not your code's."** — this is the line that tells
  ex-coders (PMs, designers, founders) that Shannon is for them too.
  Without it, the "AI Workspace" framing still reads as "AI for
  programmers".
- **"30+ events"** in section 1 cards — the actual count is 30 (see
  the E4 hook audit), but "+ events" reads better than exactly 30. We
  don't need to update this when we add event #31.
- **"Pluggable by design"** in section 4 — sets up the G1/G2 pluggable
  MCP story before the user clicks into Integrations.

## What to avoid

- **No emoji.** Looks unprofessional; Material icons are right there.
- **No testimonials.** We don't have name-brand users yet; shipping
  fake testimonials is worse than shipping none.
- **No "Trusted by [grayscale logos]"** strip. Same reason.
- **No FAQ.** FAQs go on a `/help` page; on the landing page they
  distract from the CTA.
- **No dark mode toggle.** Ship one mode (dark, to match the app's
  default); respect `prefers-color-scheme` only if cheap.

## Open questions for the user

1. **Domain**: do we already own `shannon.ai`? If not, what's the
   interim host? (`shannon.dev`, `getshannon.app`, GitHub Pages on
   `shannon-desktop.github.io`?)
2. **Bilingual**: should the page ship in Chinese and English day one?
   The user is Chinese-speaking; the repositioning doc references both
   audiences. Two pages doubles copy maintenance.
3. **Brand assets**: do we have a logo? A wordmark? The page needs at
   least one consistent brand mark.
4. **Download links**: where do installers actually live? GitHub
   Releases is the obvious choice; do we want a CDN in front?
5. **Analytics**: do we want privacy-respecting analytics (Plausible /
   Fathom) or none? The decision affects the cookie banner question.

## Implementation phasing

If approved:

1. **Copy + structure** (no design). Plain HTML, content-only. Half a
   day.
2. **Visual pass** (Tailwind, icons, one hero image). One day.
3. **Deploy pipeline** (DNS, CDN, certs). Half a day.
4. **Integration logos + cross-links**. Half a day.

Total: 2-3 days. Can be cut after phase 1 (a plain page is better than
no page).

## References

- `docs/architecture/tasks-vs-mission-control.md` — the four-card
  structure in section 2 mirrors the three-surface distinction there
- Sprint 1 brand work (sidebar four-card layout) — the card icons and
  one-liners should match the sidebar verbatim
- `README.md` — long-form version of the same content; the landing page
  is the elevator pitch, the README is the technical pitch
