import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Badge } from '@/components/ui/badge'

describe('Badge', () => {
  it('renders children', () => {
    render(<Badge>Active</Badge>)
    expect(screen.getByText('Active')).toBeInTheDocument()
  })

  it('applies variant classes', () => {
    const { container } = render(<Badge variant="error">Failed</Badge>)
    const badge = container.querySelector('[data-slot="badge"]')!
    expect(badge.className).toContain('bg-error/10')
    expect(badge.className).toContain('text-error')
  })

  it('applies size sm classes', () => {
    const { container } = render(<Badge size="sm">x</Badge>)
    expect(container.querySelector('[data-slot="badge"]')!.className).toContain('text-[10px]')
  })

  it('defaults to neutral variant', () => {
    const { container } = render(<Badge>x</Badge>)
    const cls = container.querySelector('[data-slot="badge"]')!.className
    expect(cls).toContain('bg-surface-container')
    expect(cls).toContain('text-on-surface-variant')
  })
})
