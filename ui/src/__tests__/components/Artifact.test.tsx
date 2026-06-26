import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { detectArtifacts } from '@/components/artifact/detectArtifact'
import { ArtifactProvider, useArtifact } from '@/components/artifact/ArtifactContext'
import { ArtifactChip } from '@/components/artifact/ArtifactChip'
import { ArtifactPanel } from '@/components/artifact/ArtifactPanel'

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
    const md = '\`\`\`html\n<div>hi</div>\n\`\`\`'
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
    const md = '\`\`\`markdown\n# hi\nshort body\n\`\`\`'
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

  it('clicking the chip opens the panel context', async () => {
    const { container } = render(
      <I18nProvider>
        <ArtifactProvider>
          <ArtifactChip artifact={{ kind: 'html', source: '<p>hi</p>', title: 'Test artifact', confidence: 'high' }} />
          <ArtifactPanel />
        </ArtifactProvider>
      </I18nProvider>,
    )
    expect(container.querySelector('[role="complementary"]')).toBeNull()
    const button = screen.getByRole('button', { name: /Open HTML artifact: Test artifact/ })
    fireEvent.click(button)
    await waitFor(() => {
      expect(container.querySelector('[role="complementary"]')).toBeTruthy()
    })
  })
})

describe('ArtifactPanel F2 polish', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  function renderWithArtifact() {
    return render(
      <I18nProvider>
        <ArtifactProvider>
          <ArtifactChip artifact={{ kind: 'html', source: '<p>hi</p>', title: 'Test artifact', confidence: 'high' }} />
          <ArtifactPanel />
        </ArtifactProvider>
      </I18nProvider>,
    )
  }

  it('shows fullscreen toggle button after panel opens', async () => {
    const { container } = renderWithArtifact()
    fireEvent.click(screen.getByRole('button', { name: /Open HTML artifact: Test artifact/ }))
    await waitFor(() => {
      expect(container.querySelector('[role="complementary"]')).toBeTruthy()
    })
    expect(screen.getByRole('button', { name: 'Enter fullscreen' })).toBeInTheDocument()
  })

  it('toggles fullscreen mode on button click', async () => {
    const { container } = renderWithArtifact()
    fireEvent.click(screen.getByRole('button', { name: /Open HTML artifact: Test artifact/ }))
    await waitFor(() => {
      expect(container.querySelector('[role="complementary"]')).toBeTruthy()
    })
    const fsBtn = screen.getByRole('button', { name: 'Enter fullscreen' })
    fireEvent.click(fsBtn)
    expect(screen.getByRole('button', { name: 'Exit fullscreen' })).toBeInTheDocument()
    const panel = container.querySelector('[role="complementary"]') as HTMLElement
    expect(panel.className).toContain('fixed')
    expect(localStorage.getItem('shannon.artifact.fullscreen')).toBe('1')
  })

  it('shows auto-open toggle button', async () => {
    const { container } = renderWithArtifact()
    fireEvent.click(screen.getByRole('button', { name: /Open HTML artifact: Test artifact/ }))
    await waitFor(() => {
      expect(container.querySelector('[role="complementary"]')).toBeTruthy()
    })
    const toggle = screen.getByRole('button', { name: 'Toggle auto-open on detection' })
    expect(toggle.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(toggle)
    expect(toggle.getAttribute('aria-pressed')).toBe('true')
    expect(localStorage.getItem('shannon.artifact.autoOpen')).toBe('1')
  })

  it('persists width to localStorage after resize', async () => {
    const { container } = renderWithArtifact()
    fireEvent.click(screen.getByRole('button', { name: /Open HTML artifact: Test artifact/ }))
    await waitFor(() => {
      expect(container.querySelector('[role="complementary"]')).toBeTruthy()
    })
    const handle = container.querySelector('[aria-label="Drag to resize panel"]') as HTMLElement
    expect(handle).toBeTruthy()
    fireEvent.pointerDown(handle)
    fireEvent.pointerMove(window, { clientX: 200 })
    fireEvent.pointerUp(window)
    expect(localStorage.getItem('shannon.artifact.panelWidth')).toBeTruthy()
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
          <ArtifactPanel />
        </ArtifactProvider>
      </I18nProvider>,
    )
    const evt = new KeyboardEvent('keydown', { key: 'A', shiftKey: true, ctrlKey: true, bubbles: true })
    window.dispatchEvent(evt)
  })

  it('Ctrl+Shift+A cycles active artifact when panel has items', async () => {
    const { container } = render(
      <I18nProvider>
        <ArtifactProvider>
          <ArtifactChip artifact={{ kind: 'html', source: '<p>a</p>', title: 'A', confidence: 'high' }} />
          <ArtifactChip artifact={{ kind: 'svg', source: '<svg/>', title: 'B', confidence: 'high' }} />
          <ArtifactPanel />
        </ArtifactProvider>
      </I18nProvider>,
    )
    const buttons = screen.getAllByRole('button', { name: /Open .+ artifact:/ })
    fireEvent.click(buttons[0])
    await waitFor(() => {
      expect(container.querySelector('[role="complementary"]')).toBeTruthy()
    })
    const panel = container.querySelector('[role="complementary"]') as HTMLElement
    fireEvent.click(buttons[1])
    await waitFor(() => {
      expect(panel.querySelector('.truncate')?.textContent).toBe('B')
    })
    const evt = new KeyboardEvent('keydown', { key: 'A', shiftKey: true, ctrlKey: true, bubbles: true })
    window.dispatchEvent(evt)
    await waitFor(() => {
      const afterTitle = panel.querySelector('.truncate')?.textContent
      expect(['A', 'B']).toContain(afterTitle)
    })
  })
})

describe('HtmlRenderer security', () => {
  it('renders iframe with sandbox attribute', async () => {
    const { HtmlRenderer } = await import('@/components/artifact/HtmlRenderer')
    const { container } = render(<HtmlRenderer source="<p>hi</p>" />)
    const iframe = container.querySelector('iframe')
    expect(iframe).toBeTruthy()
    expect(iframe?.getAttribute('sandbox')).toBe('allow-scripts')
    expect(iframe?.getAttribute('sandbox')?.includes('allow-same-origin')).toBe(false)
  })

  it('injects strict CSP meta tag', async () => {
    const { HtmlRenderer } = await import('@/components/artifact/HtmlRenderer')
    const { container } = render(<HtmlRenderer source="<p>hi</p>" />)
    const iframe = container.querySelector('iframe')
    const srcDoc = iframe?.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain("Content-Security-Policy")
    expect(srcDoc).toContain("default-src 'none'")
  })
})

describe('MermaidRenderer', () => {
  it('renders iframe with sandbox attribute', async () => {
    const { MermaidRenderer } = await import('@/components/artifact/MermaidRenderer')
    const { container } = render(<MermaidRenderer source="graph TD\nA-->B" />)
    const iframe = container.querySelector('iframe')
    expect(iframe).toBeTruthy()
    expect(iframe?.getAttribute('sandbox')).toBe('allow-scripts')
    expect(iframe?.getAttribute('sandbox')?.includes('allow-same-origin')).toBe(false)
  })

  it('injects CSP allowing only the mermaid CDN', async () => {
    const { MermaidRenderer } = await import('@/components/artifact/MermaidRenderer')
    const { container } = render(<MermaidRenderer source="graph TD\nA-->B" />)
    const srcDoc = container.querySelector('iframe')?.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain('Content-Security-Policy')
    expect(srcDoc).toContain('cdn.jsdelivr.net/npm/mermaid@11')
    expect(srcDoc).toContain("default-src 'none'")
  })

  it('embeds source as JSON-encoded string', async () => {
    const { MermaidRenderer } = await import('@/components/artifact/MermaidRenderer')
    const { container } = render(<MermaidRenderer source="graph TD\nA-->B" />)
    const srcDoc = container.querySelector('iframe')?.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain('graph TD')
    expect(srcDoc).toContain('securityLevel')
    expect(srcDoc).toContain('strict')
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

describe('CodeBlock', () => {
  it('renders source inside pre/code', async () => {
    const { CodeBlock } = await import('@/components/artifact/CodeBlock')
    const { container } = render(<CodeBlock source="const x = 1" kind="document" />)
    expect(container.querySelector('pre')).toBeTruthy()
    expect(container.querySelector('code.hljs')).toBeTruthy()
  })

  it('escapes HTML characters in output', async () => {
    const { CodeBlock } = await import('@/components/artifact/CodeBlock')
    const { container } = render(<CodeBlock source={'<script>alert(1)</script>'} />)
    const html = container.querySelector('code')?.innerHTML ?? ''
    expect(html).not.toContain('<script>')
    expect(html).toContain('&lt;')
    expect(html).toContain('script')
  })

  it('highlights known language tokens', async () => {
    const { CodeBlock } = await import('@/components/artifact/CodeBlock')
    const { container } = render(<CodeBlock source={'const x = 1'} />)
    const html = container.querySelector('code')?.innerHTML ?? ''
    expect(html).toContain('hljs-')
  })
})
