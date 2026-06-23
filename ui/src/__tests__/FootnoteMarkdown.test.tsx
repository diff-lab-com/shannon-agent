import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { FootnoteMarkdown } from '@/components/chat/FootnoteMarkdown'

describe('FootnoteMarkdown', () => {
  it('renders plain markdown unchanged when no footnotes are present', () => {
    const { container } = render(<FootnoteMarkdown>{'Hello **world**'}</FootnoteMarkdown>)
    expect(container.textContent).toContain('Hello world')
    expect(container.querySelector('footer')).toBeNull()
  })

  it('extracts [^id]: definitions and renders them as a footnotes list', () => {
    const md = [
      'See the docs[^1] for more.',
      '',
      '[^1]: https://example.com/docs',
    ].join('\n')
    render(<FootnoteMarkdown>{md}</FootnoteMarkdown>)
    expect(screen.getByText('https://example.com/docs')).toBeInTheDocument()
    expect(screen.getAllByLabelText('Footnote 1').length).toBeGreaterThanOrEqual(1)
  })

  it('removes definition lines from the body', () => {
    const md = [
      'Intro[^a].',
      '',
      '[^a]: def line',
    ].join('\n')
    const { container } = render(<FootnoteMarkdown>{md}</FootnoteMarkdown>)
    const footer = container.querySelector('footer')
    expect(footer).not.toBeNull()
    expect(footer?.textContent).toContain('def line')
  })

  it('supports multiple distinct footnote ids', () => {
    const md = [
      'First[^x] and second[^y].',
      '',
      '[^x]: one',
      '[^y]: two',
    ].join('\n')
    render(<FootnoteMarkdown>{md}</FootnoteMarkdown>)
    expect(screen.getByText('one')).toBeInTheDocument()
    expect(screen.getByText('two')).toBeInTheDocument()
    expect(screen.getAllByLabelText('Footnote x').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByLabelText('Footnote y').length).toBeGreaterThanOrEqual(1)
  })
})
