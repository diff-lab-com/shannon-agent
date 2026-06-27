import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { Banner } from '@/components/ui/banner'

describe('Banner', () => {
  it('renders children in a role=status surface (info tone)', () => {
    render(<Banner tone="info">All good</Banner>)
    expect(screen.getByRole('status')).toHaveTextContent('All good')
  })

  it('uses role=alert for the error tone', () => {
    render(<Banner tone="error">Something broke</Banner>)
    expect(screen.getByRole('alert')).toHaveTextContent('Something broke')
  })

  it('omits the dismiss button when onDismiss is not provided', () => {
    render(<Banner tone="info">No dismiss</Banner>)
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('renders a dismiss button labelled by dismissLabel that fires onDismiss', () => {
    const onDismiss = vi.fn()
    render(
      <Banner tone="info" onDismiss={onDismiss} dismissLabel="Dismiss">
        Dismissible
      </Banner>,
    )
    fireEvent.click(screen.getByLabelText('Dismiss'))
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })

  it('applies card-variant and error-tone classes', () => {
    render(<Banner variant="card" tone="error">x</Banner>)
    const el = screen.getByRole('alert')
    expect(el.className).toContain('rounded-xl')
    expect(el.className).toContain('bg-error-container/40')
  })

  it('defaults to bar variant + info tone', () => {
    render(<Banner>default</Banner>)
    const el = screen.getByRole('status')
    expect(el.className).toContain('border-b')
    expect(el.className).toContain('bg-secondary-container/40')
  })
})
