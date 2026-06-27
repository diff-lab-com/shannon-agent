import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Textarea } from '@/components/ui/textarea'

describe('Textarea', () => {
  it('renders with placeholder', () => {
    render(<Textarea placeholder="Type here" />)
    expect(screen.getByPlaceholderText('Type here')).toBeInTheDocument()
  })

  it('applies focus ring classes', () => {
    render(<Textarea placeholder="x" aria-label="notes" />)
    const el = screen.getByLabelText('notes')
    expect(el.className).toContain('focus-visible:ring-2')
    expect(el.className).toContain('focus-visible:border-ring')
  })

  it('calls onChange when typing', async () => {
    const user = userEvent.setup()
    let value = ''
    render(<Textarea aria-label="notes" onChange={(e) => (value = e.target.value)} />)
    await user.type(screen.getByLabelText('notes'), 'hi')
    expect(value).toBe('hi')
  })

  it('respects disabled state', () => {
    render(<Textarea aria-label="notes" disabled />)
    expect(screen.getByLabelText('notes')).toBeDisabled()
  })
})
