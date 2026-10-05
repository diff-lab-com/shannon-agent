// Settings R3 T11 (C6) — ToolGroupCard: collapsed header (icon + i18n title
// + count badge + first/last tool-name summary), expand renders the original
// tool cards in place, collapse hides them again.

import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ToolGroupCard from '@/components/chat/ToolGroupCard'
import type { ToolCall } from '@/types'

function card(id: string, name = 'read_file') {
  return { tool_use_id: id, tool_name: name, tool_input: {}, status: 'completed' as const }
}

function bubble(ui: React.ReactElement) {
  return render(<I18nProvider>{ui}</I18nProvider>)
}

beforeEach(() => {
  localStorage.clear()
})

describe('ToolGroupCard', () => {
  it('renders the collapsed header with the i18n title, count badge and summary', () => {
    bubble(
      <ToolGroupCard kind="explore" count={3} firstToolName="Read" lastToolName="Glob">
        <div>card-a</div>
      </ToolGroupCard>,
    )
    const header = screen.getByTestId('tool-group-header')
    expect(header).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByTestId('tool-group-card')).toHaveAttribute('data-group-kind', 'explore')
    expect(screen.getByText('Explore tools')).toBeInTheDocument()
    expect(screen.getByTestId('tool-group-count')).toHaveTextContent('3 calls')
    const summary = screen.getByTestId('tool-group-summary')
    expect(summary).toHaveTextContent('Read')
    expect(summary).toHaveTextContent('Glob')
    // Collapsed by default — the folded cards are NOT in the DOM.
    expect(screen.queryByTestId('tool-group-body')).toBeNull()
  })

  it('shows only the first tool name when the run is single-named', () => {
    bubble(
      <ToolGroupCard kind="changes" count={2} firstToolName="write_file" lastToolName="write_file">
        <div>card-a</div>
      </ToolGroupCard>,
    )
    const summary = screen.getByTestId('tool-group-summary')
    expect(summary).toHaveTextContent('write_file')
    expect(summary.textContent).not.toContain('…')
  })

  it('expands on click, renders the original cards in place, then collapses', () => {
    const calls: ToolCall[] = [card('a'), card('b')]
    bubble(
      <ToolGroupCard kind="terminal" count={2} firstToolName="Bash" lastToolName="bash">
        {calls.map(tc => (
          <div key={tc.tool_use_id} data-testid={`tool-card-${tc.tool_use_id}`}>
            {tc.tool_use_id}
          </div>
        ))}
      </ToolGroupCard>,
    )
    expect(screen.queryByTestId('tool-card-a')).toBeNull()
    fireEvent.click(screen.getByTestId('tool-group-header'))
    expect(screen.getByTestId('tool-group-header')).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByTestId('tool-card-a')).toBeInTheDocument()
    expect(screen.getByTestId('tool-card-b')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('tool-group-header'))
    expect(screen.queryByTestId('tool-card-a')).toBeNull()
  })

  it('renders each kind\u2019s distinct title and icon', () => {
    const { unmount } = bubble(<ToolGroupCard kind="explore" count={2}><div /></ToolGroupCard>)
    expect(screen.getByText('Explore tools')).toBeInTheDocument()
    unmount()
    bubble(<ToolGroupCard kind="terminal" count={2}><div /></ToolGroupCard>)
    expect(screen.getByText('Terminal commands')).toBeInTheDocument()
    unmount()
    bubble(<ToolGroupCard kind="changes" count={2}><div /></ToolGroupCard>)
    expect(screen.getByText('File changes')).toBeInTheDocument()
  })
})
