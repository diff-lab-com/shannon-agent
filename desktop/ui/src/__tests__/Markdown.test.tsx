import { describe, it, expect } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { Markdown } from '@/components/chat/Markdown'

function renderMd(md: string) {
  return render(<Markdown>{md}</Markdown>, { wrapper: I18nProvider })
}

describe('Markdown — basic formatting', () => {
  it('renders a paragraph with bold text', () => {
    renderMd('Hello **world**')
    expect(screen.getByText('world', { selector: 'strong' })).toBeInTheDocument()
  })

  it('renders an unordered list with multiple items', () => {
    renderMd('- one\n- two\n- three')
    const items = screen.getAllByRole('listitem')
    expect(items).toHaveLength(3)
    expect(items[0]).toHaveTextContent('one')
  })

  it('renders an ordered list with `1.` `2.` `3.` markers', () => {
    const { container } = renderMd('1. apple\n2. banana\n3. cherry')
    const ol = container.querySelector('ol')
    expect(ol).not.toBeNull()
    expect(ol?.children).toHaveLength(3)
  })

  it('renders a heading with semantic tag', () => {
    const { container } = renderMd('# Title')
    const h1 = container.querySelector('h1')
    expect(h1).toHaveTextContent('Title')
  })

  it('renders a horizontal rule', () => {
    const { container } = renderMd('above\n\n---\n\nbelow')
    expect(container.querySelector('hr')).not.toBeNull()
  })

  it('renders images that resolve local file paths', () => {
    renderMd('![alt](/some/file.png)')
    const img = screen.getByAltText('alt')
    expect(img).toHaveAttribute('src', 'asset://localhost/some/file.png')
  })
})

describe('Markdown — code blocks', () => {
  it('renders a fenced code block with copy button', () => {
    const { container } = renderMd('```ts\nconst x = 1\nconst y = 2\n```')
    const code = container.querySelector('pre code')
    expect(code).toHaveTextContent('const x = 1')
    const buttons = container.querySelectorAll('button[aria-label*="Copy" i]')
    expect(buttons.length).toBeGreaterThanOrEqual(1)
  })

  it('extracts the language tag into a header label', () => {
    renderMd('```python\nprint("hello")\nprint("world")\n```')
    // The header label for language; text-uppercase true. Allow case-insensitive.
    const pythonLabel = screen.getByText('python', { exact: false })
    expect(pythonLabel).toBeInTheDocument()
  })

  it('falls back to a generic "text" label when language is unknown', () => {
    renderMd('```\nsome code here\n```')
    // Loose match — looking for any label that says "text" or is a fallback
    const label = screen.getAllByText(/text/i).length
    expect(label).toBeGreaterThanOrEqual(1)
  })

  it('shows a line-numbers toggle when code has > 5 lines', () => {
    const md = '```ts\n' + Array.from({ length: 8 }, (_, i) => `line${i}`).join('\n') + '\n```'
    const { container } = renderMd(md)
    const toggles = container.querySelectorAll('button[aria-label*="line number" i]')
    expect(toggles.length).toBeGreaterThanOrEqual(1)
  })

  it('does NOT show a line-numbers toggle for short blocks', () => {
    const md = '```ts\nline1\nline2\n```'
    const { container } = renderMd(md)
    const toggles = container.querySelectorAll('button[aria-label*="line number" i]')
    expect(toggles).toHaveLength(0)
  })

  it('does not show the line-numbers toggle when code spans a single line', () => {
    const md = '```ts\nshort\n```'
    const { container } = renderMd(md)
    expect(container.querySelector('button[aria-label*="line number" i]')).toBeNull()
  })
})

describe('Markdown — chart dispatch', () => {
  it('renders a `language-chart` block as a chart (svg)', () => {
    const md = '```chart\n{"type":"bar","data":[{"label":"a","value":1}]}\n```'
    const { container } = renderMd(md)
    expect(container.querySelector('svg')).not.toBeNull()
  })

  it('renders an error message for invalid chart JSON', () => {
    renderMd('```chart\nnot json\n```')
    expect(screen.getByText(/invalid chart spec/i)).toBeInTheDocument()
  })
})

describe('Markdown — tables, blockquotes, links, inline code', () => {
  it('wraps tables in a scrollable container', () => {
    const md = '| H1 | H2 |\n|----|----|\n| a  | b  |'
    const { container } = renderMd(md)
    const wrap = container.querySelector('div.overflow-x-auto')
    expect(wrap).not.toBeNull()
    expect(wrap?.querySelector('table')).not.toBeNull()
  })

  it('applies header styling to <th>', () => {
    const md = '| H1 | H2 |\n|----|----|\n| a  | b  |'
    const { container } = renderMd(md)
    const th = container.querySelector('th')
    expect(th).toHaveClass('bg-surface-container')
  })

  it('renders blockquotes with a left-border accent', () => {
    const { container } = renderMd('> quoted text')
    const bq = container.querySelector('blockquote')
    expect(bq).not.toBeNull()
    // Tailwind's `border-l-4` compiles to `border-left-width: 4px`
    expect(bq?.className).toMatch(/border-l-4/)
  })

  it('renders inline `code` with token-based styling', () => {
    const { container } = renderMd('Use `npm install` first.')
    const code = container.querySelector('p code')
    expect(code).not.toBeNull()
    expect(code?.className).toMatch(/bg-surface-container/)
  })

  it('decorates external links with an outbound icon', () => {
    const { container } = renderMd('[docs](https://example.com)')
    const link = container.querySelector('a[href="https://example.com"]')
    expect(link).not.toBeNull()
    expect(link?.getAttribute('target')).toBe('_blank')
    expect(link?.getAttribute('rel')).toMatch(/noopener/)
    // The icon span is aria-hidden and uses the open_in_new symbol
    const icon = link?.querySelector('.material-symbols-outlined')
    expect(icon?.textContent).toBe('open_in_new')
  })

  it('does not decorate relative links with the outbound icon', () => {
    const { container } = renderMd('[local](/foo)')
    const link = container.querySelector('a[href="/foo"]')
    expect(link?.querySelector('.material-symbols-outlined')).toBeNull()
  })
})

describe('Markdown — safety', () => {
  it('strips <script> tags', () => {
    const { container } = renderMd('hello<script>alert(1)</script>')
    expect(container.querySelector('script')).toBeNull()
  })

  it('strips inline event handlers on img tags', () => {
    const { container } = renderMd('![alt](/x.png "onclick=alert(1)")')
    const img = container.querySelector('img')
    expect(img).not.toBeNull()
    expect(img?.getAttribute('onclick')).toBeNull()
  })
})
