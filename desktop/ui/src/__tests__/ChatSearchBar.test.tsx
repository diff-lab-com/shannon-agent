// B1 §4-12 — in-conversation search bar: match counting over user +
// assistant text, Enter/Shift+Enter navigation with virtualizer jumps and
// flash notifications, Esc/close (restores composer focus), and the
// disabled no-match state.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import ChatSearchBar from '@/pages/chat/ChatSearchBar'
import type { ChatMessage } from '@/types'

const msg = (role: ChatMessage['role'], content: string, ts = 1): ChatMessage => ({
  role,
  content,
  timestamp: ts,
})

const baseMessages: ChatMessage[] = [
  msg('user', 'What is a red panda?'),
  msg('assistant', 'A red panda is a small arboreal mammal.'),
  msg('tool', 'tool payload should never match'),
  msg('user', 'Where do they live?'),
  msg('assistant', 'Red pandas live in the eastern Himalayas.'),
]

function makeVirtualizer() {
  return { scrollToIndex: vi.fn() }
}

function renderBar(overrides: Partial<Parameters<typeof ChatSearchBar>[0]> = {}) {
  const virtualizer = makeVirtualizer()
  const onFlash = vi.fn()
  const onClose = vi.fn()
  const props = {
    messages: baseMessages,
    virtualized: true,
    virtualizer,
    scrollParentRef: { current: null },
    onFlash,
    onClose,
    ...overrides,
  }
  render(<ChatSearchBar {...props} />)
  return { virtualizer, onFlash, onClose, props }
}

beforeEach(() => {
  localStorage.clear()
})

describe('ChatSearchBar — matching and counting', () => {
  it('shows 0 with disabled navigation when the query has no matches', () => {
    renderBar()
    const input = screen.getByLabelText('Search in conversation')
    fireEvent.change(input, { target: { value: 'zebra' } })
    expect(screen.getByTestId('chat-search-count')).toHaveTextContent('0')
    expect(screen.getByLabelText('Next match (Enter)')).toBeDisabled()
    expect(screen.getByLabelText('Previous match (Shift+Enter)')).toBeDisabled()
  })

  it('counts case-insensitive matches over user + assistant text only', () => {
    renderBar()
    fireEvent.change(screen.getByLabelText('Search in conversation'), { target: { value: 'RED PANDA' } })
    // two user/assistant messages mention it; the tool payload never counts
    expect(screen.getByTestId('chat-search-count')).toHaveTextContent('1/3')
  })
})

describe('ChatSearchBar — navigation', () => {
  it('jumps to the first match on query and walks with Enter (virtualized)', () => {
    const { virtualizer, onFlash } = renderBar()
    const input = screen.getByLabelText('Search in conversation')
    fireEvent.change(input, { target: { value: 'panda' } })
    expect(virtualizer.scrollToIndex).toHaveBeenLastCalledWith(0, { align: 'center' })
    expect(onFlash).toHaveBeenLastCalledWith(0)
    expect(screen.getByTestId('chat-search-count')).toHaveTextContent('1/3')

    fireEvent.keyDown(input, { key: 'Enter' })
    expect(virtualizer.scrollToIndex).toHaveBeenLastCalledWith(1, { align: 'center' })
    expect(onFlash).toHaveBeenLastCalledWith(1)
    expect(screen.getByTestId('chat-search-count')).toHaveTextContent('2/3')

    // wraps forward at the end
    fireEvent.keyDown(input, { key: 'Enter' })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(screen.getByTestId('chat-search-count')).toHaveTextContent('1/3')

    // Shift+Enter walks backwards (wraps to the last match)
    fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
    expect(screen.getByTestId('chat-search-count')).toHaveTextContent('3/3')
  })

  it('scrolls the DOM node in non-virtualized mode', () => {
    const parent = document.createElement('div')
    const target = document.createElement('div')
    // "Himalayas" matches the message at index 4.
    target.setAttribute('data-message-index', '4')
    parent.appendChild(target)
    document.body.appendChild(parent)
    const { virtualizer } = renderBar({
      virtualized: false,
      scrollParentRef: { current: parent },
    })
    fireEvent.change(screen.getByLabelText('Search in conversation'), { target: { value: 'Himalayas' } })
    expect(virtualizer.scrollToIndex).not.toHaveBeenCalled()
    expect(target.scrollIntoView).toHaveBeenCalledWith({ block: 'center' })
    parent.remove()
  })
})

describe('ChatSearchBar — close behavior', () => {
  it('Escape closes, notifies flash cleanup, and refocuses the composer', () => {
    const onClose = vi.fn()
    const focusSpy = vi.fn()
    window.addEventListener('shannon:focus-composer', focusSpy)
    const { onFlash } = renderBar({ onClose })
    const input = screen.getByLabelText('Search in conversation')
    fireEvent.change(input, { target: { value: 'panda' } })
    fireEvent.keyDown(input, { key: 'Escape' })
    expect(onFlash).toHaveBeenLastCalledWith(null)
    expect(onClose).toHaveBeenCalled()
    expect(focusSpy).toHaveBeenCalled()
    window.removeEventListener('shannon:focus-composer', focusSpy)
  })

  it('the ✕ button closes as well', () => {
    const onClose = vi.fn()
    renderBar({ onClose })
    fireEvent.click(screen.getByLabelText('Close search'))
    expect(onClose).toHaveBeenCalled()
  })

  it('prev/next buttons navigate like the keys', () => {
    renderBar()
    fireEvent.change(screen.getByLabelText('Search in conversation'), { target: { value: 'panda' } })
    fireEvent.click(screen.getByLabelText('Next match (Enter)'))
    expect(screen.getByTestId('chat-search-count')).toHaveTextContent('2/3')
    fireEvent.click(screen.getByLabelText('Previous match (Shift+Enter)'))
    expect(screen.getByTestId('chat-search-count')).toHaveTextContent('1/3')
  })
})
