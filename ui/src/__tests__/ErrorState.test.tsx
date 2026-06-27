import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import ErrorState from '@/components/ui/error-state'

describe('ErrorState', () => {
  it('renders with role=alert', () => {
    render(<ErrorState title="Something broke" />)
    expect(screen.getByRole('alert')).toBeInTheDocument()
    expect(screen.getByText('Something broke')).toBeInTheDocument()
  })

  it('uses default error icon when none provided', () => {
    render(<ErrorState title="Fail" />)
    expect(document.querySelector('.material-symbols-outlined')?.textContent).toBe('error')
  })

  it('honors custom icon', () => {
    render(<ErrorState title="Fail" icon="cloud_off" />)
    expect(document.querySelector('.material-symbols-outlined')?.textContent).toBe('cloud_off')
  })

  it('renders description when provided', () => {
    render(<ErrorState title="Fail" description="Network timeout" />)
    expect(screen.getByText('Network timeout')).toBeInTheDocument()
  })

  it('invokes action callback on click', () => {
    const onClick = vi.fn()
    render(<ErrorState title="Fail" action={{ label: 'Retry', onClick }} />)
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(onClick).toHaveBeenCalledOnce()
  })

  it('omits action button when no action', () => {
    render(<ErrorState title="Fail" />)
    expect(screen.queryByRole('button')).toBeNull()
  })
})
