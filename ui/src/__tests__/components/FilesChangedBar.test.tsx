import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { MessageBubble } from '@/components/chat/MessageBubble'
import type { ChatMessage } from '@/types'

vi.mock('@/context/AppContext', () => ({
  useApp: () => ({
    sendMessage: vi.fn(),
    currentSessionId: null,
    switchSession: vi.fn(),
    refreshSessions: vi.fn(),
  }),
}))

const wrap = (ui: React.ReactNode) => <MemoryRouter>{ui}</MemoryRouter>

const baseMsg = (overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  id: 'm1',
  role: 'assistant',
  content: 'ok',
  timestamp: new Date().toISOString(),
  ...overrides,
} as ChatMessage)

describe('MessageBubble — files-changed summary bar (D5)', () => {
  it('shows the summary bar when a completed write_file tool call exists', () => {
    render(wrap(
      <MessageBubble
        message={baseMsg({
          tool_calls: [{
            tool_use_id: 'tc1',
            tool_name: 'write_file',
            status: 'completed',
            tool_input: { file_path: '/repo/src/app.ts', content: 'x' },
            result: 'wrote',
          }],
        })}
        messageIndex={0}
        onViewDiff={() => {}}
      />
    ))
    expect(screen.getByText('1 file changed')).toBeInTheDocument()
    expect(screen.getByText('/repo/src/app.ts')).toBeInTheDocument()
  })

  it('does not show summary bar for non-file-mutating tools', () => {
    render(wrap(
      <MessageBubble
        message={baseMsg({
          tool_calls: [{
            tool_use_id: 'tc1',
            tool_name: 'bash',
            status: 'completed',
            tool_input: { cmd: 'ls' },
            result: 'output',
          }],
        })}
        messageIndex={0}
        onViewDiff={() => {}}
      />
    ))
    expect(screen.queryByText(/file changed/)).not.toBeInTheDocument()
  })

  it('does not show summary bar when write_file is still running', () => {
    render(wrap(
      <MessageBubble
        message={baseMsg({
          tool_calls: [{
            tool_use_id: 'tc1',
            tool_name: 'write_file',
            status: 'running',
            tool_input: { file_path: '/repo/x.ts' },
          }],
        })}
        messageIndex={0}
        onViewDiff={() => {}}
      />
    ))
    expect(screen.queryByText(/file changed/)).not.toBeInTheDocument()
  })

  it('does not show summary bar when write_file errored', () => {
    render(wrap(
      <MessageBubble
        message={baseMsg({
          tool_calls: [{
            tool_use_id: 'tc1',
            tool_name: 'write_file',
            status: 'completed',
            is_error: true,
            tool_input: { file_path: '/repo/x.ts' },
            result: 'permission denied',
          }],
        })}
        messageIndex={0}
        onViewDiff={() => {}}
      />
    ))
    expect(screen.queryByText(/file changed/)).not.toBeInTheDocument()
  })

  it('renders Review all button when 2+ files changed', () => {
    const onMulti = vi.fn()
    render(wrap(
      <MessageBubble
        message={baseMsg({
          tool_calls: [
            { tool_use_id: 'tc1', tool_name: 'write_file', status: 'completed', tool_input: { file_path: '/a.ts' }, result: 'ok' },
            { tool_use_id: 'tc2', tool_name: 'edit_file', status: 'completed', tool_input: { file_path: '/b.ts' }, result: 'ok' },
          ],
        })}
        messageIndex={0}
        onViewDiff={() => {}}
        onViewDiffMulti={onMulti}
      />
    ))
    expect(screen.getByText('2 files changed')).toBeInTheDocument()
    fireEvent.click(screen.getByText('Review all'))
    expect(onMulti).toHaveBeenCalledWith(['/a.ts', '/b.ts'])
  })

  it('dedupes repeated paths', () => {
    render(wrap(
      <MessageBubble
        message={baseMsg({
          tool_calls: [
            { tool_use_id: 'tc1', tool_name: 'write_file', status: 'completed', tool_input: { file_path: '/a.ts' }, result: 'ok' },
            { tool_use_id: 'tc2', tool_name: 'edit_file', status: 'completed', tool_input: { file_path: '/a.ts' }, result: 'ok' },
          ],
        })}
        messageIndex={0}
        onViewDiff={() => {}}
        onViewDiffMulti={() => {}}
      />
    ))
    expect(screen.getByText('1 file changed')).toBeInTheDocument()
  })

  it('does not render Review all button when only 1 file', () => {
    render(wrap(
      <MessageBubble
        message={baseMsg({
          tool_calls: [{
            tool_use_id: 'tc1',
            tool_name: 'write_file',
            status: 'completed',
            tool_input: { file_path: '/x.ts' },
            result: 'ok',
          }],
        })}
        messageIndex={0}
        onViewDiff={() => {}}
        onViewDiffMulti={() => {}}
      />
    ))
    expect(screen.queryByText('Review all')).not.toBeInTheDocument()
  })
})
