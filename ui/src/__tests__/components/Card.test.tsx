import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'

describe('Card', () => {
  it('renders children', () => {
    render(
      <Card>
        <div>content</div>
      </Card>
    )
    expect(screen.getByText('content')).toBeInTheDocument()
  })

  it('applies elevated variant classes', () => {
    const { container } = render(<Card variant="elevated">x</Card>)
    const card = container.querySelector('[data-slot="card"]')!
    expect(card.className).toContain('bg-surface-container-lowest')
    expect(card.className).toContain('border')
  })

  it('applies glass variant', () => {
    const { container } = render(<Card variant="glass">x</Card>)
    expect(container.querySelector('[data-slot="card"]')!.className).toContain('glass-card')
  })

  it('interactive adds hover classes', () => {
    const { container } = render(<Card interactive>x</Card>)
    const card = container.querySelector('[data-slot="card"]')!
    expect(card.className).toContain('hover:border-primary/40')
  })

  it('renders header/title subcomponents', () => {
    render(
      <Card>
        <CardHeader>
          <CardTitle>My Title</CardTitle>
        </CardHeader>
        <CardContent>body</CardContent>
      </Card>
    )
    expect(screen.getByText('My Title')).toBeInTheDocument()
    expect(screen.getByText('body')).toBeInTheDocument()
  })

  it('applies padding token', () => {
    const { container } = render(<Card padding="xl">x</Card>)
    expect(container.querySelector('[data-slot="card"]')!.className).toContain('p-xl')
  })
})
