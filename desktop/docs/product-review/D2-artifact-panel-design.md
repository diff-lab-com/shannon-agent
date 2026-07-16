# D2 — Artifact Panel Design

**Status:** Planning / not yet implemented
**Owner:** ed
**Last updated:** 2026-06-26
**Estimated effort:** 3–5 days

## Why

Claude Desktop's Artifacts feature changed user expectations for what
an AI assistant can *produce* — not just text answers but interactive
HTML, React components, SVG diagrams, Mermaid charts, documents. The
`05c` competitive analysis flags this as the second-biggest consumer-
grade gap after voice mode.

Shannon currently renders markdown responses only. Any structured
output (a chart, a form, a runnable snippet) is lost inside a code
block. The Artifact Panel closes this gap by detecting artifact-
eligible content in responses and rendering it in a dedicated side
panel.

## Scope (in / out)

**In scope (MVP):**
- Detect artifact-eligible code blocks in assistant responses (HTML,
  SVG, Mermaid, Markdown documents, JSON data)
- Render HTML/SVG safely in a sandboxed iframe with strict CSP
- Render Mermaid diagrams via the `mermaid` library
- Render Markdown documents as formatted HTML
- Side panel opens automatically when an artifact is produced
- User can close panel; reopen from a chip in the message
- "Copy" and "Export" actions for each artifact

**Out of scope (MVP):**
- Live React/TSX rendering (needs Babel-in-browser, sandboxing is hard)
- Multi-file projects (Claude-style "create a React app")
- Real-time collaboration / sharing
- Version history of artifact edits
- Custom artifact types (PDF, spreadsheets)

## UX design

### Detection

When an assistant message is rendered, MessageBubble scans `tool_calls`
and content for artifact signals:

| Signal | Artifact type | Confidence |
|--------|---------------|------------|
| ` ```html ... ``` ` block ≥20 lines | html | High |
| ` ```mermaid ... ``` ` block | mermaid | High |
| ` ```svg ... ``` ` block | svg | High |
| `write_file` tool call with `.html`/`.svg`/`.mmd` extension | (type matches ext) | High |
| Long markdown block (≥500 words, has headings) | document | Medium — needs user confirm |

When confidence is Medium, a chip appears: "View as document?" — user
clicks to promote to an artifact.

### Side panel layout

```
┌─ Chat (flex-1) ────────────────────┐ ┌─ Artifact (right) ──┐
│                                    │ │ [Tabs: Preview|Code]│
│  Message with artifact chip ────┐  │ │ ━━━━━━━━━━━━━━━━━ │
│                                 │  │ │                    │
│  > "Here's the form..."         │  │ │  [Rendered iframe] │
│                                 │  │ │                    │
│  [📊 View chart] ←── chip ──────┼──┼─│  ...                │
│                                 │  │ │                    │
│  Next message...                │  │ │ ━━━━━━━━━━━━━━━━━ │
│                                 │  │ │ [Copy] [Export]    │
└──────────────────────────────────┘ └────────────────────┘
```

- Panel width: 40% of window, min 480px, max 720px
- Collapsible to a right-edge tab
- Multiple artifacts in one response → tabs within the panel

### MessageBubble changes

When a message produces an artifact, MessageBubble shows a chip below
the response:

```
[icon] Generated artifact: Sales-chart.html    [Open ↗]
```

Clicking "Open" expands the side panel (or reuses the open one).

### Tabs within panel

Each artifact has two tabs:
- **Preview** — rendered HTML / SVG / Mermaid
- **Code** — the raw source with syntax highlighting + copy button

### Actions

- **Copy** — copies raw source to clipboard
- **Export** — save-as dialog: `.html` / `.svg` / `.mmd` / `.md`
- **Open externally** — opens in default OS app (uses existing
  `convertFileSrc` + save-to-temp pattern)

## Technical design

### Sandboxed iframe (security)

The HTML/SVG renderer MUST run in a sandboxed iframe with strict CSP:

```html
<iframe
  sandbox="allow-scripts"
  srcdoc="..."
  csp="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'"
/>
```

- No `allow-same-origin` — the iframe cannot access parent window,
  cookies, localStorage, or Shannon's Tauri API
- No external network — `default-src 'none'` blocks all fetches
- `allow-scripts` only — inline scripts can run for interactivity
- Rendered content is wrapped in `<pre>` if it fails to parse

### Component layer

```
ui/src/components/artifact/
├── ArtifactPanel.tsx       # right-side panel container
├── ArtifactChip.tsx        # chip shown in MessageBubble
├── ArtifactTabs.tsx        # tab switching (preview/code)
├── renderers/
│   ├── HtmlRenderer.tsx    # sandboxed iframe
│   ├── SvgRenderer.tsx     # img with data URL (or iframe)
│   ├── MermaidRenderer.tsx # mermaid library
│   └── DocumentRenderer.tsx# markdown → HTML
└── detectArtifact.ts       # heuristics for code-block scanning
```

### State

`ArtifactContext` provides:
- `artifacts: Artifact[]` — currently open
- `activeArtifactId: string | null`
- `openArtifact(artifact)`
- `closeArtifact(id)`
- `closeAll()`

State is ephemeral (not persisted) — artifacts are tied to messages,
so on session reload they reappear via message scanning.

### Performance

- Lazy-load Mermaid (≥500KB) only when first mermaid artifact appears
- Iframe `srcdoc` capped at 1MB — larger content falls back to code
  view with a warning
- Virtualize tab list if a single message produces >5 artifacts

## Phased plan

### Phase 1 — HTML/SVG rendering (1.5 days)

- ArtifactPanel layout + chip + state
- HtmlRenderer with sandboxed iframe
- SvgRenderer as `<img src="data:image/svg+xml;...">` (no script execution)
- Auto-open panel when message contains qualifying HTML/SVG
- Copy + Export actions

Exit: Any HTML or SVG the assistant generates can be previewed live.

### Phase 2 — Mermaid + documents (1 day)

- Add MermaidRenderer (lazy-loaded)
- Add DocumentRenderer for long markdown blocks
- Chip confidence heuristic refined
- Multiple-artifact tabs

Exit: Diagrams render natively. Markdown-heavy responses promote to
document view.

### Phase 3 — UX polish (1 day)

- Resizable panel drag handle
- Fullscreen artifact view
- "Open externally" action
- Keyboard shortcut: `Cmd/Ctrl + Shift + A` toggles panel
- Settings: "Auto-open artifact panel" toggle (default on)

Exit: Feels native and polished.

### Phase 4 — Live React (future sprint, 2+ days)

- Sandboxed Babel transform
- Limited to known-safe component patterns
- Requires CDN script approval review

Out of MVP scope but the architecture doesn't preclude it.

## Risks

- **XSS via inline scripts in HTML.** The sandboxed iframe with no
  `allow-same-origin` blocks parent-window access, but the rendered
  content can still run arbitrary scripts within the iframe. That's
  the intended behavior (interactive demos) but means we cannot
  ingest untrusted HTML from external sources via this path.
- **Mermaid bundle size.** ~500KB after gzip. Acceptable but lazy-load
  is mandatory.
- **Content Security Policy conflicts with Tauri's default.** Verify
  the iframe CSP doesn't conflict with the app-wide Tauri CSP.
- **Assistant produces malformed HTML.** Renderer must fail gracefully
  to a code-view fallback.

## Acceptance criteria

- [ ] When the assistant writes a complete HTML document in a response,
      an artifact chip appears and the side panel shows the rendered
      output on click
- [ ] Mermaid blocks render as diagrams, not as code
- [ ] SVG blocks render as images
- [ ] Copy button copies the raw source
- [ ] Export saves to disk via native save dialog
- [ ] Panel can be collapsed and reopened
- [ ] No script in rendered HTML can access Shannon's APIs, cookies,
      localStorage, or the parent window (verified via test)

## Dependencies

- npm: `mermaid` (add to `ui/package.json`)
- Tauri: already has `dialog` plugin for save-as
- Engine: none (artifact detection is UI-side only)

## Open questions

- Should artifact state persist across sessions? (Current proposal: no,
  regenerate from message scan.)
- Should we auto-export to `~/.shannon/artifacts/` for later reference?
- How do artifacts interact with the existing Diff Preview infrastructure?
  They're both "side panel shows derived content" — could share state.
