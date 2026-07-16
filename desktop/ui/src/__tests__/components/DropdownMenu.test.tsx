import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'

const items: DropdownMenuItem[] = [
  { id: 'edit', label: 'Edit', icon: 'edit' },
  { id: 'duplicate', label: 'Duplicate' },
  { id: 'delete', label: 'Delete', destructive: true },
  { id: 'disabled', label: 'Disabled', disabled: true },
]

describe('DropdownMenu', () => {
  it('does not render when closed', () => {
    render(<DropdownMenu open={false} onClose={() => {}} items={items} />)
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  })

  it('renders items when open', () => {
    render(<DropdownMenu open={true} onClose={() => {}} items={items} ariaLabel="Actions" />)
    expect(screen.getByRole('menu', { name: 'Actions' })).toBeInTheDocument()
    expect(screen.getByText('Edit')).toBeInTheDocument()
    expect(screen.getByText('Delete')).toBeInTheDocument()
  })

  it('renders icons when provided', () => {
    render(<DropdownMenu open={true} onClose={() => {}} items={items} />)
    expect(document.querySelector('.material-symbols-outlined')).toBeInTheDocument()
  })

  it('fires onSelect and onClose on click', () => {
    const onSelect = vi.fn()
    const onClose = vi.fn()
    render(
      <DropdownMenu
        open={true}
        onClose={onClose}
        items={[{ id: 'x', label: 'X', onSelect }, ...items]}
      />
    )
    fireEvent.click(screen.getByText('X'))
    expect(onSelect).toHaveBeenCalledOnce()
    expect(onClose).toHaveBeenCalledOnce()
  })

  it('fires onClose on Escape', () => {
    const onClose = vi.fn()
    render(<DropdownMenu open={true} onClose={onClose} items={items} />)
    fireEvent.keyDown(document.body, { key: 'Escape' })
    expect(onClose).toHaveBeenCalled()
  })

  it('arrow down moves focus to next enabled item', () => {
    render(<DropdownMenu open={true} onClose={() => {}} items={items} />)
    const editBtn = screen.getByText('Edit').closest('button')!
    fireEvent.mouseEnter(editBtn)
    fireEvent.keyDown(document.body, { key: 'ArrowDown' })
    expect(screen.getByText('Duplicate').closest('button')).toHaveFocus()
  })

  it('marks disabled items', () => {
    render(<DropdownMenu open={true} onClose={() => {}} items={items} />)
    expect(screen.getByText('Disabled').closest('button')).toBeDisabled()
  })

  it('marks destructive items with error class', () => {
    render(<DropdownMenu open={true} onClose={() => {}} items={items} />)
    const deleteBtn = screen.getByText('Delete').closest('button')!
    expect(deleteBtn.className).toContain('text-error')
  })

  it('closes on outside click', () => {
    const onClose = vi.fn()
    render(
      <div>
        <button>outside</button>
        <DropdownMenu open={true} onClose={onClose} items={items} />
      </div>
    )
    fireEvent.mouseDown(screen.getByText('outside'))
    expect(onClose).toHaveBeenCalled()
  })
})
