// P2.3 ResultRoutingEditor tests.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import ResultRoutingEditor, { encodeChannel, parseChannel } from '@/components/tasks/ResultRoutingEditor'

describe('encodeChannel / parseChannel helpers', () => {
  it('encodes slack with target', () => {
    expect(encodeChannel('slack', '#ops')).toBe('slack:#ops')
  })
  it('encodes email with target', () => {
    expect(encodeChannel('email', 'a@b.com')).toBe('email:a@b.com')
  })
  it('returns bare kind for notification and log', () => {
    expect(encodeChannel('notification', '')).toBe('notification')
    expect(encodeChannel('log', '')).toBe('log')
  })
  it('returns empty when target missing for slack/email', () => {
    expect(encodeChannel('slack', '  ')).toBe('')
  })
  it('parses slack entry back to kind+target', () => {
    expect(parseChannel('slack:#ops')).toEqual({ kind: 'slack', target: '#ops' })
  })
  it('parses notification as no-target', () => {
    expect(parseChannel('notification')).toEqual({ kind: 'notification', target: '' })
  })
  it('returns null for unknown kind', () => {
    expect(parseChannel('foo:bar')).toBeNull()
  })
})

describe('ResultRoutingEditor UI', () => {
  it('renders configured channels from value', () => {
    render(<ResultRoutingEditor value={['slack:#ops', 'notification']} onChange={() => {}} />)
    expect(screen.getByText('slack: #ops')).toBeInTheDocument()
    // 'notification' channel renders in the configured list with a remove button next to it
    const list = screen.getByLabelText('Configured channels')
    expect(list).toHaveTextContent('Notification')
  })

  it('shows empty hint when no channels', () => {
    render(<ResultRoutingEditor value={[]} onChange={() => {}} />)
    expect(screen.getByText(/No channels configured/i)).toBeInTheDocument()
  })

  it('emits updated list when Add clicked (slack channel)', () => {
    const onChange = vi.fn()
    render(<ResultRoutingEditor value={[]} onChange={onChange} />)
    fireEvent.change(screen.getByLabelText('Channel type'), { target: { value: 'slack' } })
    fireEvent.change(screen.getByLabelText('slack target'), { target: { value: '#release' } })
    fireEvent.click(screen.getByText('Add'))
    expect(onChange).toHaveBeenCalledWith(['slack:#release'])
  })

  it('hides target input for notification and Add is enabled without target', () => {
    const onChange = vi.fn()
    render(<ResultRoutingEditor value={[]} onChange={onChange} />)
    fireEvent.change(screen.getByLabelText('Channel type'), { target: { value: 'notification' } })
    expect(screen.queryByLabelText('notification target')).not.toBeInTheDocument()
    fireEvent.click(screen.getByText('Add'))
    expect(onChange).toHaveBeenCalledWith(['notification'])
  })

  it('removes a channel via the Remove button', () => {
    const onChange = vi.fn()
    render(<ResultRoutingEditor value={['log', 'slack:#ops']} onChange={onChange} />)
    fireEvent.click(screen.getByLabelText('Remove log'))
    expect(onChange).toHaveBeenCalledWith(['slack:#ops'])
  })

  it('does not add duplicates', () => {
    const onChange = vi.fn()
    render(<ResultRoutingEditor value={['log']} onChange={onChange} />)
    fireEvent.change(screen.getByLabelText('Channel type'), { target: { value: 'log' } })
    fireEvent.click(screen.getByText('Add'))
    expect(onChange).not.toHaveBeenCalled()
  })

  it('disables Add button when target required but empty', () => {
    render(<ResultRoutingEditor value={[]} onChange={() => {}} />)
    // default pendingKind is slack; no target typed → button disabled
    expect(screen.getByText('Add')).toBeDisabled()
  })
})
