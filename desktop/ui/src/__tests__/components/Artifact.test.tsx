import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { ThemeProvider } from '@/context/ThemeContext'
import { detectArtifacts } from '@/components/artifact/detectArtifact'
import { ArtifactProvider, useArtifact } from '@/components/artifact/ArtifactContext'
import { ArtifactChip } from '@/components/artifact/ArtifactChip'

// Review §P2-17: mock the lazily-imported bundled mermaid so these tests
// don't pull the real (heavy) mermaid bundle into jsdom. `vi.hoisted` is
// required because vi.mock factories are hoisted above file-level consts.
const { mermaidMock } = vi.hoisted(() => ({
  mermaidMock: { initialize: vi.fn(), render: vi.fn() },
}))
vi.mock('mermaid', () => ({ default: mermaidMock }))

/** Batch D4: ArtifactPanel was retired — the dock (RightDock) hosts open
 *  artifacts, so these tests observe the context through a probe. */
function ArtifactProbe() {
  const { artifacts, activeId } = useArtifact()
  return <div data-testid="artifact-probe" data-count={artifacts.length} data-active={activeId ?? ''} />
}

const HTML_FIXTURE = `<!DOCTYPE html>
<html>
<head><title>My Form</title></head>
<body>
<form>
<input name="q" placeholder="Search">
<button type="submit">Go</button>
</form>
</body>
</html>`

const SVG_FIXTURE = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
<circle cx="50" cy="50" r="40" fill="red" />
</svg>`

const MERMAID_FIXTURE = `graph TD
A[Start] --> B{Decision}
B -->|Yes| C[Action 1]
B -->|No| D[Action 2]`

describe('detectArtifacts', () => {
  it('detects HTML code fence with 5+ lines', () => {
    const md = `\`\`\`html\n${HTML_FIXTURE}\n\`\`\``
    const out = detectArtifacts(md)
    expect(out).toHaveLength(1)
    expect(out[0].kind).toBe('html')
    expect(out[0].title).toBe('My Form')
    expect(out[0].confidence).toBe('high')
  })

  it('ignores short HTML (< 5 lines)', () => {
    const md = '```html\n<div>hi</div>\n```'
    expect(detectArtifacts(md)).toHaveLength(0)
  })

  it('detects SVG code fence', () => {
    const md = `\`\`\`svg\n${SVG_FIXTURE}\n\`\`\``
    const out = detectArtifacts(md)
    expect(out).toHaveLength(1)
    expect(out[0].kind).toBe('svg')
  })

  it('detects SVG without explicit lang tag', () => {
    const md = `\`\`\`\n${SVG_FIXTURE}\n\`\`\``
    const out = detectArtifacts(md)
    expect(out).toHaveLength(1)
    expect(out[0].kind).toBe('svg')
  })

  it('detects mermaid code fence', () => {
    const md = `\`\`\`mermaid\n${MERMAID_FIXTURE}\n\`\`\``
    const out = detectArtifacts(md)
    expect(out).toHaveLength(1)
    expect(out[0].kind).toBe('mermaid')
  })

  it('detects long markdown as document', () => {
    const body = `# Guide\n\n${'Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod. '.repeat(30)}`
    const md = `\`\`\`markdown\n${body}\n\`\`\``
    const out = detectArtifacts(md)
    expect(out).toHaveLength(1)
    expect(out[0].kind).toBe('document')
    expect(out[0].confidence).toBe('medium')
  })

  it('ignores short markdown', () => {
    const md = '```markdown\n# hi\nshort body\n```'
    expect(detectArtifacts(md)).toHaveLength(0)
  })

  it('returns empty for non-artifact content', () => {
    expect(detectArtifacts('hello world')).toHaveLength(0)
    expect(detectArtifacts('')).toHaveLength(0)
  })

  it('dedupes identical blocks', () => {
    const md = `\`\`\`svg\n${SVG_FIXTURE}\n\`\`\`\n\n\`\`\`svg\n${SVG_FIXTURE}\n\`\`\``
    expect(detectArtifacts(md)).toHaveLength(1)
  })

  it('detects multiple different artifacts', () => {
    const md = `\`\`\`svg\n${SVG_FIXTURE}\n\`\`\`\n\n\`\`\`mermaid\n${MERMAID_FIXTURE}\n\`\`\``
    expect(detectArtifacts(md)).toHaveLength(2)
  })
})

// B3 §P1-10/§P2-24: stable content-hash ids, ~~~ fences, htm tag, and
// content-derived titles for SVG/mermaid.
describe('detectArtifacts — B3 robustness', () => {
  it('assigns stable content-hash ids: same content converges, different content differs', () => {
    const md = `\`\`\`svg\n${SVG_FIXTURE}\n\`\`\``
    const [a] = detectArtifacts(md)
    const [b] = detectArtifacts(md)
    expect(a.id).toMatch(/^svg:/)
    expect(a.id).toBe(b.id)
    const [c] = detectArtifacts(`\`\`\`svg\n<svg xmlns="http://www.w3.org/2000/svg"></svg>\n\`\`\``)
    expect(c.id).not.toBe(a.id)
    // CRLF vs LF and outer whitespace are normalized away.
    const [d] = detectArtifacts(`\`\`\`svg\n${SVG_FIXTURE.replace(/\n/g, '\r\n')}\n\`\`\`\n`)
    expect(d.id).toBe(a.id)
  })

  it('detects ~~~ fences', () => {
    const md = `~~~svg\n${SVG_FIXTURE}\n~~~`
    const out = detectArtifacts(md)
    expect(out).toHaveLength(1)
    expect(out[0].kind).toBe('svg')
  })

  it('detects the htm language tag as html', () => {
    const md = '```htm\n<html><body><p>' + 'line\n'.repeat(6) + '</p></body></html>\n```'
    const out = detectArtifacts(md)
    expect(out).toHaveLength(1)
    expect(out[0].kind).toBe('html')
  })

  it('does not report a ~~~ sequence inside a ``` fence body twice', () => {
    const body = `${SVG_FIXTURE}\n~~~svg\n<svg></svg>\n~~~`
    const md = `\`\`\`svg\n${body}\n\`\`\``
    const out = detectArtifacts(md)
    expect(out).toHaveLength(1)
  })

  it('extracts an SVG title from <title> and falls back to <text>', () => {
    const withTitle = detectArtifacts('```svg\n<svg><title>Revenue 2026</title><rect/></svg>\n```')
    expect(withTitle[0].title).toBe('Revenue 2026')
    const withText = detectArtifacts('```svg\n<svg><text x="0" y="0">Quarterly Report</text></svg>\n```')
    expect(withText[0].title).toBe('Quarterly Report')
    expect(detectArtifacts('```svg\n<svg><rect/></svg>\n```')[0].title).toBe('')
  })

  it('extracts a mermaid title from the first node label', () => {
    const out = detectArtifacts('```mermaid\nflowchart LR\nA[Checkout Flow] --> B{Paid?}\n```')
    expect(out[0].title).toBe('Checkout Flow')
  })
})

describe('ArtifactContext', () => {
  function Probe() {
    const { artifacts, activeId, open, close, closeAll } = useArtifact()
    return (
      <div>
        <div data-testid="count">{artifacts.length}</div>
        <div data-testid="active">{activeId ?? 'none'}</div>
        <button onClick={() => closeAll()}>clear</button>
        <button onClick={() => open({ kind: 'svg', source: '<svg/>', title: 'X', confidence: 'high' })}>open</button>
        <button onClick={() => artifacts[0] && close(artifacts[0].id)}>closeFirst</button>
      </div>
    )
  }

  function renderProbe() {
    return render(
      <I18nProvider>
        <ArtifactProvider>
          <Probe />
        </ArtifactProvider>
      </I18nProvider>,
    )
  }

  it('starts empty with no active id', () => {
    renderProbe()
    expect(screen.getByTestId('count')).toHaveTextContent('0')
    expect(screen.getByTestId('active')).toHaveTextContent('none')
  })

  it('open() adds artifact and sets active', () => {
    renderProbe()
    fireEvent.click(screen.getByText('open'))
    expect(screen.getByTestId('count')).toHaveTextContent('1')
    expect(screen.getByTestId('active')).not.toHaveTextContent('none')
  })

  it('close() removes the artifact', () => {
    renderProbe()
    fireEvent.click(screen.getByText('open'))
    fireEvent.click(screen.getByText('closeFirst'))
    expect(screen.getByTestId('count')).toHaveTextContent('0')
  })

  it('closeAll() clears everything', () => {
    renderProbe()
    fireEvent.click(screen.getByText('open'))
    fireEvent.click(screen.getByText('open'))
    fireEvent.click(screen.getByText('clear'))
    expect(screen.getByTestId('count')).toHaveTextContent('0')
  })

  it('useArtifact throws outside provider', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    expect(() => render(<Probe />)).toThrow(/ArtifactProvider/)
    spy.mockRestore()
  })
})

// B3 §P1-14 / §P1-11: session-scoped chat tabs + provider-level auto-open
// bookkeeping that survives virtualized chip remounts.
describe('ArtifactContext — B3 scoping & autoOpen dedup', () => {
  const CHAT = { kind: 'html' as const, source: '<p>chat</p>', title: 'Chat doc', confidence: 'high' as const, origin: 'chat' as const, id: 'html:abc' }
  const DISK = { kind: 'document' as const, source: '# d', title: 'Disk doc', confidence: 'high' as const, origin: 'disk' as const, path: '/tmp/d.md', id: 'disk:/tmp/d.md' }
  const WEB = { kind: 'web' as const, source: 'https://example.com', title: 'https://example.com', confidence: 'high' as const, id: 'web:https://example.com' }

  function ScopeProbe() {
    const { artifacts, open, closeChatArtifacts } = useArtifact()
    return (
      <div>
        <div data-testid="ids">{artifacts.map(a => a.id).join(',')}</div>
        <button onClick={() => { open(CHAT); open(DISK); open(WEB) }}>seed</button>
        <button onClick={() => closeChatArtifacts()}>sweep</button>
      </div>
    )
  }

  it('closeChatArtifacts removes chat tabs, keeps disk and web tabs', () => {
    render(
      <I18nProvider>
        <ArtifactProvider>
          <ScopeProbe />
        </ArtifactProvider>
      </I18nProvider>,
    )
    fireEvent.click(screen.getByText('seed'))
    expect(screen.getByTestId('ids')).toHaveTextContent('html:abc')
    expect(screen.getByTestId('ids')).toHaveTextContent('disk:/tmp/d.md')
    expect(screen.getByTestId('ids')).toHaveTextContent('web:https://example.com')
    fireEvent.click(screen.getByText('sweep'))
    expect(screen.getByTestId('ids').textContent).toBe('disk:/tmp/d.md,web:https://example.com')
  })

  it('autoOpenOnce opens an id exactly once across chip remounts', () => {
    const artifact = { kind: 'svg' as const, source: '<svg><text>remount</text></svg>', title: 'R', confidence: 'high' as const }

    function ChipAndProbe() {
      const { autoOpenOnce, artifacts } = useArtifact()
      return (
        <div>
          <button onClick={() => autoOpenOnce(artifact)}>fire</button>
          <div data-testid="count">{artifacts.length}</div>
        </div>
      )
    }

    // Simulates two virtualization cycles of the same chip: fresh component
    // instances, same provider — the old per-mount firedRef re-fired here.
    const { rerender } = render(
      <I18nProvider>
        <ArtifactProvider>
          <ChipAndProbe />
        </ArtifactProvider>
      </I18nProvider>,
    )
    fireEvent.click(screen.getByText('fire'))
    expect(screen.getByTestId('count')).toHaveTextContent('1')
    rerender(
      <I18nProvider>
        <ArtifactProvider>
          <ChipAndProbe />
        </ArtifactProvider>
      </I18nProvider>,
    )
    fireEvent.click(screen.getByText('fire'))
    expect(screen.getByTestId('count')).toHaveTextContent('1')
  })

  it('closeChatArtifacts re-arms auto-open for a fresh session', () => {
    const artifact = { kind: 'svg' as const, source: '<svg><text>session</text></svg>', title: 'S', confidence: 'high' as const, origin: 'chat' as const }

    function ChipAndProbe() {
      const { autoOpenOnce, artifacts, closeChatArtifacts } = useArtifact()
      return (
        <div>
          <button onClick={() => autoOpenOnce(artifact)}>fire</button>
          <button onClick={() => closeChatArtifacts()}>sweep</button>
          <div data-testid="count">{artifacts.length}</div>
        </div>
      )
    }

    render(
      <I18nProvider>
        <ArtifactProvider>
          <ChipAndProbe />
        </ArtifactProvider>
      </I18nProvider>,
    )
    fireEvent.click(screen.getByText('fire'))
    expect(screen.getByTestId('count')).toHaveTextContent('1')
    // Session switch sweeps the tab — and with it the auto-open marker —
    // so the next session's identical chip may open once again.
    fireEvent.click(screen.getByText('sweep'))
    expect(screen.getByTestId('count')).toHaveTextContent('0')
    fireEvent.click(screen.getByText('fire'))
    expect(screen.getByTestId('count')).toHaveTextContent('1')
  })
})

describe('ArtifactChip', () => {
  function renderChip(kind: 'html' | 'svg' = 'html') {
    return render(
      <I18nProvider>
        <ArtifactProvider>
          <ArtifactChip artifact={{ kind, source: '<x/>', title: 'Test artifact', confidence: 'high' }} />
        </ArtifactProvider>
      </I18nProvider>,
    )
  }

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders with the artifact title', () => {
    renderChip()
    expect(screen.getByText('Test artifact')).toBeInTheDocument()
  })

  it('clicking the chip opens it in the artifact context', async () => {
    const { container } = render(
      <I18nProvider>
        <ArtifactProvider>
          <ArtifactChip artifact={{ kind: 'html', source: '<p>hi</p>', title: 'Test artifact', confidence: 'high' }} />
          <ArtifactProbe />
        </ArtifactProvider>
      </I18nProvider>,
    )
    expect(screen.getByTestId('artifact-probe')).toHaveAttribute('data-count', '0')
    const button = screen.getByRole('button', { name: /Open HTML artifact: Test artifact/ })
    fireEvent.click(button)
    await waitFor(() => {
      expect(screen.getByTestId('artifact-probe')).toHaveAttribute('data-count', '1')
    })
    expect(container).toBeTruthy()
  })
})

describe('ArtifactContext keyboard shortcut', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('Ctrl+Shift+A does nothing with no artifacts', () => {
    render(
      <I18nProvider>
        <ArtifactProvider>
          <ArtifactProbe />
        </ArtifactProvider>
      </I18nProvider>,
    )
    const evt = new KeyboardEvent('keydown', { key: 'A', shiftKey: true, ctrlKey: true, bubbles: true })
    window.dispatchEvent(evt)
    expect(screen.getByTestId('artifact-probe')).toHaveAttribute('data-count', '0')
  })

  it('Ctrl+Shift+A cycles active artifact when artifacts are open', async () => {
    render(
      <I18nProvider>
        <ArtifactProvider>
          <ArtifactChip artifact={{ kind: 'html', source: '<p>a</p>', title: 'A', confidence: 'high' }} />
          <ArtifactChip artifact={{ kind: 'svg', source: '<svg/>', title: 'B', confidence: 'high' }} />
          <ArtifactProbe />
        </ArtifactProvider>
      </I18nProvider>,
    )
    const buttons = screen.getAllByRole('button', { name: /Open .+ artifact:/ })
    fireEvent.click(buttons[0])
    await waitFor(() => {
      expect(screen.getByTestId('artifact-probe')).toHaveAttribute('data-count', '1')
    })
    fireEvent.click(buttons[1])
    await waitFor(() => {
      expect(screen.getByTestId('artifact-probe')).toHaveAttribute('data-count', '2')
    })
    const before = screen.getByTestId('artifact-probe').getAttribute('data-active')
    const evt = new KeyboardEvent('keydown', { key: 'A', shiftKey: true, ctrlKey: true, bubbles: true })
    window.dispatchEvent(evt)
    await waitFor(() => {
      const after = screen.getByTestId('artifact-probe').getAttribute('data-active')
      expect(after).toBeTruthy()
      expect(after).not.toBe(before)
    })
  })
})

describe('HtmlRenderer security', () => {
  it('renders a fully-sandboxed iframe — honest-static, no scripts (B3 §P1-9)', async () => {
    const { HtmlRenderer } = await import('@/components/artifact/HtmlRenderer')
    const { container } = render(<HtmlRenderer source="<p>hi</p>" />)
    const iframe = container.querySelector('iframe')
    expect(iframe).toBeTruthy()
    // Empty sandbox: opaque origin, script execution impossible. The old
    // `allow-scripts` only ever produced a silently static page (srcdoc
    // inherits the parent CSP), so the posture is now truthful.
    expect(iframe?.getAttribute('sandbox')).toBe('')
  })

  it('injects a strict CSP meta without script-src', async () => {
    const { HtmlRenderer } = await import('@/components/artifact/HtmlRenderer')
    const { container } = render(<HtmlRenderer source="<p>hi</p>" />)
    const iframe = container.querySelector('iframe')
    const srcDoc = iframe?.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain("Content-Security-Policy")
    expect(srcDoc).toContain("default-src 'none'")
    // The old script-src 'unsafe-inline' could never re-grant scripts
    // under the intersecting parent policy — it must stay gone.
    expect(srcDoc).not.toContain('script-src')
    expect(srcDoc).toContain('style-src')
  })

  it('injects into case-variant / attribute-carrying <head> tags once', async () => {
    const { HtmlRenderer } = await import('@/components/artifact/HtmlRenderer')
    const source = '<!DOCTYPE html><HTML><HEAD id="x"><meta charset="utf-8"></HEAD><body></body></html>'
    const { container } = render(<HtmlRenderer source={source} />)
    const srcDoc = container.querySelector('iframe')?.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain('<HEAD id="x"><meta http-equiv="Content-Security-Policy"')
    expect(srcDoc.match(/Content-Security-Policy/g)).toHaveLength(1)
  })

  it('injects after <html …> when no <head> exists', async () => {
    const { HtmlRenderer } = await import('@/components/artifact/HtmlRenderer')
    const source = '<html lang="zh"><body><p>x</p></body></html>'
    const { container } = render(<HtmlRenderer source={source} />)
    const srcDoc = container.querySelector('iframe')?.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain('<html lang="zh"><head><meta http-equiv="Content-Security-Policy"')
  })

  it('wraps fragments without any html/head scaffold', async () => {
    const { HtmlRenderer } = await import('@/components/artifact/HtmlRenderer')
    const { container } = render(<HtmlRenderer source="<p>x</p>" />)
    const srcDoc = container.querySelector('iframe')?.getAttribute('srcdoc') ?? ''
    expect(srcDoc.startsWith('<!DOCTYPE html><html><head>')).toBe(true)
    expect(srcDoc.endsWith('<p>x</p></body></html>')).toBe(true)
  })
})

// Review §P2-17: mermaid is a bundled npm dependency (lazy chunk), and the
// iframe only receives the rendered SVG under a script-less CSP — no CDN.
describe('MermaidRenderer', () => {
  beforeEach(() => {
    mermaidMock.initialize.mockReset()
    mermaidMock.render.mockReset()
    mermaidMock.render.mockResolvedValue({ svg: '<svg id="mmd-fixture"></svg>' })
  })

  it('renders a fully-sandboxed iframe (no scripts allowed)', async () => {
    const { MermaidRenderer } = await import('@/components/artifact/MermaidRenderer')
    const { container } = render(<ThemeProvider><MermaidRenderer source="graph TD\nA-->B" /></ThemeProvider>)
    const iframe = await waitFor(() => {
      const el = container.querySelector('iframe')
      expect(el).toBeTruthy()
      return el
    })
    // The srcdoc is static SVG — the sandbox must not allow scripts.
    expect(iframe?.getAttribute('sandbox')).toBe('')
  })

  it('injects a strict CSP with no script-src and no CDN', async () => {
    const { MermaidRenderer } = await import('@/components/artifact/MermaidRenderer')
    const { container } = render(<ThemeProvider><MermaidRenderer source="graph TD\nA-->B" /></ThemeProvider>)
    await waitFor(() => {
      expect(container.querySelector('iframe')).toBeTruthy()
    })
    const srcDoc = container.querySelector('iframe')?.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain('Content-Security-Policy')
    expect(srcDoc).toContain("default-src 'none'")
    expect(srcDoc).not.toContain('script-src')
    expect(srcDoc).not.toContain('cdn.jsdelivr.net')
  })

  it('renders the mermaid output (strict security level) into the iframe', async () => {
    const { MermaidRenderer } = await import('@/components/artifact/MermaidRenderer')
    const { container } = render(<ThemeProvider><MermaidRenderer source="graph TD\nA-->B" /></ThemeProvider>)
    await waitFor(() => {
      expect(mermaidMock.initialize).toHaveBeenCalledWith(
        expect.objectContaining({ securityLevel: 'strict' }),
      )
    })
    expect(mermaidMock.render).toHaveBeenCalledWith(expect.any(String), 'graph TD\\nA-->B')
    const srcDoc = container.querySelector('iframe')?.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain('<svg id="mmd-fixture"></svg>')
  })
})

describe('DocumentRenderer', () => {
  it('renders markdown paragraphs', async () => {
    const { DocumentRenderer } = await import('@/components/artifact/DocumentRenderer')
    const { container } = render(
      <I18nProvider>
        <DocumentRenderer source="Hello world" />
      </I18nProvider>
    )
    expect(container.textContent).toContain('Hello world')
  })

  it('applies remark-gfm for tables and strikethrough', async () => {
    const { DocumentRenderer } = await import('@/components/artifact/DocumentRenderer')
    const md = '| A | B |\n| - | - |\n| 1 | 2 |\n\n~~strikethrough~~'
    const { container } = render(
      <I18nProvider>
        <DocumentRenderer source={md} />
      </I18nProvider>
    )
    expect(container.querySelector('table')).toBeTruthy()
    expect(container.textContent?.includes('strikethrough')).toBeTruthy()
  })

  it('renders headings with appropriate levels', async () => {
    const { DocumentRenderer } = await import('@/components/artifact/DocumentRenderer')
    const { container } = render(
      <I18nProvider>
        <DocumentRenderer source={"# Title\n\n## Section"} />
      </I18nProvider>
    )
    expect(container.querySelector('h1')).toBeTruthy()
    expect(container.querySelector('h2')).toBeTruthy()
  })
})

describe('CodeBlock (shared primitive — batch D3)', () => {
  it('renders source inside pre/code with the language chrome', async () => {
    const { CodeBlock } = await import('@/components/code/CodeBlock')
    const { container } = render(
      <I18nProvider>
        <CodeBlock code="const x = 1" language="javascript" />
      </I18nProvider>
    )
    expect(container.querySelector('pre')).toBeTruthy()
    expect(container.querySelector('code')).toBeTruthy()
    // copy affordance lives in the header chrome
    expect(container.querySelector('button')).toBeTruthy()
  })

  it('escapes HTML characters in highlighted output', async () => {
    const { CodeBlock } = await import('@/components/code/CodeBlock')
    const { container } = render(
      <I18nProvider>
        <CodeBlock code={'<script>alert(1)</script>'} />
      </I18nProvider>
    )
    const html = container.querySelector('pre')?.innerHTML ?? ''
    expect(html).not.toContain('<script>alert')
    expect(html).toContain('&lt;')
  })
})
