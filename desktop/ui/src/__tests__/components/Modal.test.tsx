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

  it('calls onClose when clicking backdrop', () => {
    const onClose = vi.fn()
    const { container } = render(<Modal open={true} onClose={onClose} title="X"><p>y</p></Modal>)
    const backdrop = container.firstElementChild as HTMLElement
    fireEvent.click(backdrop)
    expect(onClose).toHaveBeenCalled()
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
