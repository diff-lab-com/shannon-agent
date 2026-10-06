// office Wave 3 C8 — source-pill tests for the chat markdown pipeline:
// a B3 injection line (`[Source: <name>] (<url-or-path>)`) on its own
// paragraph renders as a pill whose click opens the right wrapper; ordinary
// paragraphs, partial matches and quoted lines stay literal markdown.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { Markdown } from '@/components/chat/Markdown'
import { matchSourceLine } from '@/components/chat/SourcePill'

const { openExternal, openWithDefaultApp } = vi.hoisted(() => ({
  openExternal: vi.fn(),
  openWithDefaultApp: vi.fn(),
}))

import type TauriApiModule from '@/lib/tauri-api'
vi.mock('@/lib/tauri-api', async (importOriginal) => ({
  ...(await importOriginal<typeof TauriApiModule>()),
  openExternal,
  openWithDefaultApp,
}))

function renderMd(md: string) {
  return render(<Markdown>{md}</Markdown>)
}

beforeEach(() => {
  vi.clearAllMocks()
  openExternal.mockResolvedValue(undefined)
  openWithDefaultApp.mockResolvedValue(undefined)
})

describe('Markdown — source pills (office Wave 3 C8)', () => {
  it('renders a whole-line [Source: …] (path) paragraph as a pill, not text', () => {
    renderMd('[Source: Q3 report] (/tmp/shannon/q3.docx)')
    const pill = screen.getByTestId('source-pill')
    expect(pill).toBeInTheDocument()
    expect(pill).toHaveTextContent('Q3 report')
    // The raw bracket syntax must not leak as paragraph text.
    expect(screen.queryByText('[Source: Q3 report]')).not.toBeInTheDocument()
  })

  it('opens local-path sources through openWithDefaultApp', () => {
    renderMd('[Source: notes.md] (/home/ed/notes.md)')
    fireEvent.click(screen.getByTestId('source-pill-open'))
    expect(openWithDefaultApp).toHaveBeenCalledWith('/home/ed/notes.md')
    expect(openExternal).not.toHaveBeenCalled()
  })

  it('opens http(s) sources through openExternal', () => {
    renderMd('[Source: Pricing page] (https://example.com/pricing)')
    fireEvent.click(screen.getByTestId('source-pill-open'))
    expect(openExternal).toHaveBeenCalledWith('https://example.com/pricing')
    expect(openWithDefaultApp).not.toHaveBeenCalled()
  })

  it('renders consecutive source lines as one pill row', () => {
    renderMd('[Source: A] (/tmp/a.csv)\n[Source: B] (https://example.com/b)')
    expect(screen.getAllByTestId('source-pill')).toHaveLength(2)
  })

  it('keeps ordinary paragraphs literal — no pill, text intact', () => {
    renderMd('See [the docs](https://example.com/docs) for details.')
    expect(screen.queryByTestId('source-pill')).not.toBeInTheDocument()
    expect(screen.getByText('the docs').closest('a')).toHaveAttribute(
      'href',
      'https://example.com/docs',
    )
  })

  it('keeps a mixed paragraph (text + source line) literal', () => {
    renderMd('Summary of findings.\n[Source: report] (/tmp/report.docx)')
    expect(screen.queryByTestId('source-pill')).not.toBeInTheDocument()
    expect(screen.getByText(/Summary of findings\./)).toBeInTheDocument()
  })

  it('keeps a source line inside a blockquote literal (quotes are verbatim)', () => {
    const { container } = renderMd('> [Source: quoted] (/tmp/quoted.md)')
    expect(screen.queryByTestId('source-pill')).not.toBeInTheDocument()
    expect(container.querySelector('blockquote')?.textContent).toContain('[Source: quoted]')
  })

  it('keeps the source syntax literal inside a code fence', () => {
    const { container } = renderMd('```\n[Source: fenced] (/tmp/fenced.md)\n```')
    expect(screen.queryByTestId('source-pill')).not.toBeInTheDocument()
    expect(container.querySelector('pre')?.textContent).toContain('[Source: fenced]')
  })

  it('matchSourceLine rejects partial matches and sentences', () => {
    expect(matchSourceLine('cited from [Source: x] (/tmp/x)')).toBeNull()
    expect(matchSourceLine('[Source: x] (/tmp/x) extra')).toBeNull()
    expect(matchSourceLine('plain line')).toBeNull()
    expect(matchSourceLine('[Source: x] (/tmp/x)')).toEqual({ name: 'x', target: '/tmp/x' })
  })
})
