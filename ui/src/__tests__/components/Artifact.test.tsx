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
