// DeleteSessionModal (B4 P2-6) — zero-coverage until R2. The confirm names
// its target, stays open while the delete is in flight (pending disables
// both actions), and the archived 永久删除 variant carries the starker copy.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import DeleteSessionModal from '@/pages/chat/DeleteSessionModal'

// The modal takes the app's translate fn as a prop — hand it one that
// resolves the ids the assertions anchor on (same ids the real locales use).
const t = (id: string, values?: Record<string, string>) => {
  const strings: Record<string, string> = {
    'chat.delete.title': 'Delete Chat',
    'chat.delete.confirm': 'Delete “{title}”? The chat and its history will be permanently removed.',
    'chat.delete.permanent.title': 'Permanently delete chat',
    'chat.delete.permanent.confirm': 'Permanently delete “{title}”? Archived chats have no trash — this cannot be undone.',
    'chat.delete.working': 'Deleting…',
    'chat.delete.cancel': 'Cancel',
    'chat.delete.confirmButton': 'Delete',
  }
  let out = strings[id] ?? id
  if (values?.title) out = out.replaceAll('{title}', values.title)
  return out
}

describe('DeleteSessionModal', () => {
  const base = { t, pending: false }

  it('renders nothing while no delete is pending', () => {
    const { container } = render(
      <DeleteSessionModal {...base} target={null} onCancel={() => {}} onConfirm={() => {}} />,
    )
    expect(screen.queryByRole('alertdialog')).toBeNull()
    expect(container).toBeEmptyDOMElement()
  })

  it('names the target in the confirm copy', () => {
    render(
      <DeleteSessionModal
        {...base}
        target={{ id: 's1', title: 'Refactor notes' }}
        onCancel={() => {}}
        onConfirm={() => {}}
      />,
    )
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    expect(screen.getByText('Delete Chat')).toBeInTheDocument()
    expect(
      screen.getByText('Delete “Refactor notes”? The chat and its history will be permanently removed.'),
    ).toBeInTheDocument()
  })

  it('cancel closes and confirm deletes', () => {
    const onCancel = vi.fn()
    const onConfirm = vi.fn()
    render(
      <DeleteSessionModal
        {...base}
        target={{ id: 's1', title: 'x' }}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(onCancel).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByTestId('delete-session-confirm'))
    expect(onConfirm).toHaveBeenCalledTimes(1)
  })

  it('pending disables both actions and relabels the confirm', () => {
    const onCancel = vi.fn()
    const onConfirm = vi.fn()
    render(
      <DeleteSessionModal
        {...base}
        pending
        target={{ id: 's1', title: 'x' }}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    )
    const confirm = screen.getByTestId('delete-session-confirm')
    expect(confirm).toBeDisabled()
    expect(confirm).toHaveAttribute('aria-busy', 'true')
    expect(confirm).toHaveTextContent('Deleting…')
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled()
    fireEvent.click(confirm)
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(onConfirm).not.toHaveBeenCalled()
    expect(onCancel).not.toHaveBeenCalled()
  })

  it('the permanent (archived) variant carries the starker copy', () => {
    render(
      <DeleteSessionModal
        {...base}
        target={{ id: 's1', title: 'Old chat', permanent: true }}
        onCancel={() => {}}
        onConfirm={() => {}}
      />,
    )
    expect(screen.getByText('Permanently delete chat')).toBeInTheDocument()
    expect(
      screen.getByText('Permanently delete “Old chat”? Archived chats have no trash — this cannot be undone.'),
    ).toBeInTheDocument()
  })
})
