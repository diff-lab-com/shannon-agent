import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'

describe('ConfirmDialog', () => {
  it('renders nothing when open=false', () => {
    const { container } = render(
      <ConfirmDialog
        open={false}
        title="t"
        message="m"
        confirmLabel="ok"
        cancelLabel="no"
        onConfirm={() => {}}
        onCancel={() => {}}
      />
    )
    expect(container.firstChild).toBeNull()
  })

  it('renders alertdialog with title + message when open', () => {
    render(
      <ConfirmDialog
        open
        title="Delete?"
        message="This cannot be undone"
        confirmLabel="Delete"
        cancelLabel="Cancel"
        onConfirm={() => {}}
        onCancel={() => {}}
      />
    )
    expect(screen.getByRole('alertdialog')).toHaveAttribute('aria-label', 'Delete?')
    expect(screen.getByText('This cannot be undone')).toBeInTheDocument()
  })

  it('invokes onConfirm when confirm button clicked', () => {
    const onConfirm = vi.fn()
    render(
      <ConfirmDialog
        open
        title="t"
        message="m"
        confirmLabel="OK"
        cancelLabel="Cancel"
        onConfirm={onConfirm}
        onCancel={() => {}}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: 'OK' }))
    expect(onConfirm).toHaveBeenCalledOnce()
  })

  it('invokes onCancel when cancel button clicked', () => {
    const onCancel = vi.fn()
    render(
      <ConfirmDialog
        open
        title="t"
        message="m"
        confirmLabel="OK"
        cancelLabel="Cancel"
        onConfirm={() => {}}
        onCancel={onCancel}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(onCancel).toHaveBeenCalledOnce()
  })

  it('cancel button is not blocked by busy state when not busy', () => {
    render(
      <ConfirmDialog
        open
        title="t"
        message="m"
        confirmLabel="OK"
        cancelLabel="Cancel"
        onConfirm={() => {}}
        onCancel={() => {}}
      />
    )
    expect(screen.getByRole('button', { name: 'Cancel' })).not.toBeDisabled()
  })

  it('confirm button shows ellipsis and is disabled when busy', () => {
    render(
      <ConfirmDialog
        open
        busy
        title="t"
        message="m"
        confirmLabel="OK"
        cancelLabel="Cancel"
        onConfirm={() => {}}
        onCancel={() => {}}
      />
    )
    const confirmBtn = screen.getByRole('button', { name: '…' })
    expect(confirmBtn).toBeDisabled()
  })
})
