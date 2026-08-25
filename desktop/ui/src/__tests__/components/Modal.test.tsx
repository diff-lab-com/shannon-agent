import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'

describe('Modal', () => {
  it('does not render when closed', () => {
    render(<Modal open={false} onClose={() => {}}><p>hidden</p></Modal>)
    expect(screen.queryByText('hidden')).not.toBeInTheDocument()
  })

  it('renders title when open', () => {
    render(<Modal open={true} onClose={() => {}} title="Confirm"><p>body</p></Modal>)
    expect(screen.getByText('Confirm')).toBeInTheDocument()
    expect(screen.getByText('body')).toBeInTheDocument()
  })

  it('calls onClose on escape', async () => {
    const user = userEvent.setup()
    const onClose = vi.fn()
    render(<Modal open={true} onClose={onClose} title="X"><p>y</p></Modal>)
    await user.type(document.body, '{Escape}')
    expect(onClose).toHaveBeenCalled()
  })

  it('does not close on escape when busy', () => {
    const onClose = vi.fn()
    render(<Modal open={true} onClose={onClose} title="X" busy><p>y</p></Modal>)
    fireEvent.keyDown(document.body, { key: 'Escape' })
    expect(onClose).not.toHaveBeenCalled()
  })

  // Backdrop-click close runs through Base UI's dismiss layer (useDismiss
  // in @base-ui/react/floating-ui-react), which registers pointerdown +
  // click listeners on `document` (capture phase) and detects outside
  // presses via the event target. The pointer/click pair below on
  // document.body is the outside-press shape; the same interaction is
  // additionally locked in e2e/modals.spec.ts against a real Chromium.
  it('closes when pressing outside the popup (backdrop)', () => {
    const onClose = vi.fn()
    render(<Modal open={true} onClose={onClose} title="X"><p>y</p></Modal>)
    fireEvent.pointerDown(document.body)
    fireEvent.pointerUp(document.body)
    fireEvent.click(document.body)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('does not close on outside press when closeOnBackdrop is false', () => {
    const onClose = vi.fn()
    render(<Modal open={true} onClose={onClose} title="X" closeOnBackdrop={false}><p>y</p></Modal>)
    fireEvent.pointerDown(document.body)
    fireEvent.pointerUp(document.body)
    fireEvent.click(document.body)
    expect(onClose).not.toHaveBeenCalled()
  })

  it('does not close on escape when closeOnEscape is false', () => {
    const onClose = vi.fn()
    render(<Modal open={true} onClose={onClose} title="X" closeOnEscape={false}><p>y</p></Modal>)
    fireEvent.keyDown(document.body, { key: 'Escape' })
    expect(onClose).not.toHaveBeenCalled()
  })

  it('renders role alertdialog when specified', () => {
    render(<Modal open={true} onClose={() => {}} title="X" role="alertdialog"><p>y</p></Modal>)
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
  })

  it('does not close when clicking inside', () => {
    const onClose = vi.fn()
    render(<Modal open={true} onClose={onClose} title="X"><p>y</p></Modal>)
    fireEvent.click(screen.getByText('y'))
    expect(onClose).not.toHaveBeenCalled()
  })

  it('locks body scroll when open', () => {
    const { unmount } = render(<Modal open={true} onClose={() => {}} title="X"><p>y</p></Modal>)
    expect(document.body.style.overflow).toBe('hidden')
    unmount()
  })

  it('restores body scroll on close', () => {
    const prev = document.body.style.overflow
    document.body.style.overflow = 'auto'
    const { rerender } = render(<Modal open={true} onClose={() => {}} title="X"><p>y</p></Modal>)
    rerender(<Modal open={false} onClose={() => {}} title="X"><p>y</p></Modal>)
    expect(document.body.style.overflow).toBe('auto')
    document.body.style.overflow = prev
  })

  it('close button calls onClose', () => {
    const onClose = vi.fn()
    render(<Modal open={true} onClose={onClose} title="X"><p>y</p></Modal>)
    fireEvent.click(screen.getByLabelText('Close'))
    expect(onClose).toHaveBeenCalled()
  })

  it('renders ModalBody and ModalFooter', () => {
    render(
      <Modal open={true} onClose={() => {}} title="X">
        <ModalBody>body content</ModalBody>
        <ModalFooter>
          <Button>OK</Button>
        </ModalFooter>
      </Modal>
    )
    expect(screen.getByText('body content')).toBeInTheDocument()
    expect(screen.getByText('OK')).toBeInTheDocument()
  })

  it('renders with aria-modal and role dialog', () => {
    render(<Modal open={true} onClose={() => {}} title="X"><p>y</p></Modal>)
    expect(screen.getByRole('dialog')).toHaveAttribute('aria-modal', 'true')
  })
})
