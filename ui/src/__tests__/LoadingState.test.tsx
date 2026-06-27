import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import LoadingState from '@/components/ui/loading-state'

describe('LoadingState', () => {
  it('renders spinner with role=status', () => {
    render(<LoadingState />)
    expect(screen.getByRole('status')).toBeInTheDocument()
  })

  it('renders label when provided', () => {
    render(<LoadingState label="Loading analytics…" />)
    expect(screen.getByText('Loading analytics…')).toBeInTheDocument()
  })

  it('omits label paragraph when not provided', () => {
    render(<LoadingState />)
    const status = screen.getByRole('status')
    expect(status.querySelector('p')).toBeNull()
  })

  it('applies size classes', () => {
    const { rerender } = render(<LoadingState size="lg" />)
    expect(screen.getByRole('status').className).toContain('py-3xl')
    rerender(<LoadingState size="sm" />)
    expect(screen.getByRole('status').className).toContain('py-sm')
  })
})
