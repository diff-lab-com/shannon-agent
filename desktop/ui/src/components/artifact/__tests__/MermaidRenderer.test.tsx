import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { ThemeProvider } from '@/context/ThemeContext'
import { buildSrcDoc, MermaidRenderer } from '../MermaidRenderer'

/**
 * Review §P2-17: mermaid is vendored via npm and lazily imported — the
 * srcdoc iframe only ever receives the *rendered* SVG under a CSP with no
 * script-src at all. The CDN-based implementation failed under the
 * production CSP (srcdoc inherits the parent CSP, which allows no
 * cdn.jsdelivr.net) and broke offline.
 */

const mermaidMock = {
  initialize: vi.fn(),
  render: vi.fn(),
}

vi.mock('mermaid', () => ({ default: mermaidMock }))

beforeEach(() => {
  mermaidMock.initialize.mockReset()
  mermaidMock.render.mockReset()
})

describe('buildSrcDoc (rendered-SVG document)', () => {
  it('embeds the rendered SVG as static markup', () => {
    const src = buildSrcDoc('<svg id="mmd-1"><g>ok</g></svg>', 'light')
    expect(src).toContain('<div id="container"><svg id="mmd-1"><g>ok</g></svg></div>')
  })

  it('declares a CSP with no script-src so nothing can execute', () => {
    const src = buildSrcDoc('<svg></svg>', 'dark')
    expect(src).toContain('Content-Security-Policy')
    expect(src).toContain("default-src 'none'")
    expect(src).not.toContain('script-src')
    // No CDN, no scripts, no <script> anywhere in the document.
    expect(src).not.toContain('cdn.jsdelivr.net')
    expect(src).not.toContain('<script')
  })

  it('still embeds theme mode', () => {
    expect(buildSrcDoc('<svg></svg>', 'dark')).toContain('data-mode="dark"')
    expect(buildSrcDoc('<svg></svg>', 'light')).toContain('data-mode="light"')
  })
})

describe('MermaidRenderer (bundled mermaid, lazy import)', () => {
  it('renders the mermaid output into a scriptless sandboxed iframe', async () => {
    mermaidMock.render.mockResolvedValue({ svg: '<svg id="mmd-x"></svg>' })
    const { container } = render(
      <ThemeProvider>
        <MermaidRenderer source="graph TD; A-->B;" />
      </ThemeProvider>,
    )
    // initialize gets strict securityLevel (theme follows the resolved mode,
    // which jsdom's matchMedia mock leaves environment-dependent)…
    await waitFor(() => {
      expect(mermaidMock.initialize).toHaveBeenCalled()
    })
    expect(mermaidMock.initialize).toHaveBeenCalledWith(
      expect.objectContaining({ startOnLoad: false, securityLevel: 'strict' }),
    )
    // …the bundled module received the diagram source…
    expect(mermaidMock.render).toHaveBeenCalledWith(expect.any(String), 'graph TD; A-->B;')
    // …and the iframe appears once rendering resolves.
    const iframe = await waitFor(() => {
      const el = container.querySelector('iframe')
      expect(el).toBeTruthy()
      return el!
    })
    expect(iframe.getAttribute('sandbox')).toBe('')
    const srcDoc = iframe.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain('<svg id="mmd-x"></svg>')
    expect(srcDoc).toContain("default-src 'none'")
    expect(srcDoc).not.toContain('<script')
    expect(srcDoc).not.toContain('cdn.jsdelivr.net')
  })

  it('shows the failure box when mermaid rejects, without mounting an iframe', async () => {
    mermaidMock.render.mockRejectedValue(new Error('Parse error on line 2'))
    const { container, getByRole } = render(
      <ThemeProvider>
        <MermaidRenderer source="graph TD; A--" />
      </ThemeProvider>,
    )
    await waitFor(() => {
      expect(getByRole('alert').textContent).toContain('Parse error on line 2')
    })
    expect(container.querySelector('iframe')).toBeNull()
  })

  it('renders a loading state before mermaid resolves', () => {
    mermaidMock.render.mockReturnValue(new Promise(() => {}))
    const { container, getByRole } = render(
      <ThemeProvider>
        <MermaidRenderer source="graph TD; A-->B;" />
      </ThemeProvider>,
    )
    expect(getByRole('status')).toBeTruthy()
    expect(container.querySelector('iframe')).toBeNull()
  })
})
