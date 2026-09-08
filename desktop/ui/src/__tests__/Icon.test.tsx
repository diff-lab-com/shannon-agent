import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Icon } from '@/components/ui/icon'

describe('Icon', () => {
  it('renders the Material Symbols name as text content', () => {
    render(<Icon name="close" />)
    const el = screen.getByText('close')
    expect(el.tagName.toLowerCase()).toBe('span')
    expect(el).toHaveClass('material-symbols-outlined')
  })

  it('defaults to aria-hidden when no aria-label is supplied', () => {
    render(<Icon name="add" />)
    expect(screen.getByText('add')).toHaveAttribute('aria-hidden', 'true')
  })

  it('drops aria-hidden when aria-label is supplied (informative)', () => {
    render(<Icon name="add" aria-label="Add item" />)
    const el = screen.getByLabelText('Add item')
    expect(el).not.toHaveAttribute('aria-hidden')
  })

  it('applies the size utility class (icon-md → 20px)', () => {
    render(<Icon name="settings" size="md" />)
    expect(screen.getByText('settings')).toHaveClass('icon-md')
  })

  it('forwards className for composition', () => {
    render(<Icon name="warning" className="text-error" />)
    expect(screen.getByText('warning')).toHaveClass('text-error')
  })
})