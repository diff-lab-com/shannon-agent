import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { CodeBlock } from '@/components/code/CodeBlock'

describe('CodeBlock — self-highlighting', () => {
  it('renders highlighted code inside pre/code', () => {
    const { container } = render(<CodeBlock code="const x = 1" language="typescript" />)
    const pre = container.querySelector('pre')
    expect(pre).toBeInTheDocument()
    expect(pre?.querySelector('code.hljs')).toBeInTheDocument()
    expect(pre?.textContent).toContain('const x = 1')
  })

  it('shows the language label in the header', () => {
    render(<CodeBlock code="x" language="python" />)
    expect(screen.getByText('python')).toBeInTheDocument()
  })

  it('escapes HTML in the fallback path instead of injecting it', () => {
    const { container } = render(<CodeBlock code="<script>alert(1)</script>" language="no-such-lang-xyz" />)
    // highlightAuto still runs for unknown languages; the dangerous input
    // must never produce a real script element either way.
    expect(container.querySelector('script')).toBeNull()
    expect(container.querySelector('pre')?.textContent).toContain('<script>')
  })
})

describe('CodeBlock — chrome', () => {
  it('hides the header when chrome={false} (artifact mode)', () => {
    render(<CodeBlock code="x" chrome={false} />)
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('copies code from the header button', () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.assign(navigator, { clipboard: { writeText } })
    render(<CodeBlock code="copy-me" language="text" />)
    fireEvent.click(screen.getByRole('button', { name: /copy code/i }))
    expect(writeText).toHaveBeenCalledWith('copy-me')
  })

  it('shows the line-number toggle only past 5 lines', () => {
    const short = render(<CodeBlock code={'a\nb\nc'} language="text" />)
    expect(short.container.querySelectorAll('button[aria-label*="line number" i]')).toHaveLength(0)
    const long = render(<CodeBlock code={Array.from({ length: 8 }, (_, i) => `l${i}`).join('\n')} language="text" />)
    expect(long.container.querySelectorAll('button[aria-label*="line number" i]')).toHaveLength(1)
  })
})

describe('CodeBlock — line-number gutter', () => {
  it('injects a scoped gutter when lineNumbers is forced on', () => {
    const { container } = render(<CodeBlock code={'a\nb\nc'} language="text" lineNumbers={true} chrome={false} />)
    const gutter = container.querySelector('pre > .line-number-row')
    expect(gutter).not.toBeNull()
    expect(gutter?.children).toHaveLength(3)
  })

  it('injects the gutter after toggling from the header', () => {
    const code = Array.from({ length: 8 }, (_, i) => `l${i}`).join('\n')
    const { container } = render(<CodeBlock code={code} language="text" />)
    expect(container.querySelector('.line-number-row')).toBeNull()
    fireEvent.click(container.querySelector('button[aria-label*="line number" i]')!)
    expect(container.querySelectorAll('.line-number-row > span')).toHaveLength(8)
  })

  it('never injects the gutter when lineNumbers={false}', () => {
    const code = Array.from({ length: 8 }, (_, i) => `l${i}`).join('\n')
    const { container } = render(<CodeBlock code={code} language="text" lineNumbers={false} />)
    // no toggle button and no gutter, regardless of line count
    expect(container.querySelectorAll('button[aria-label*="line number" i]')).toHaveLength(0)
    expect(container.querySelector('.line-number-row')).toBeNull()
  })
})
