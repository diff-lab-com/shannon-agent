// Settings R3 T11 (C6) — MessageBubble grouping integration: committed
// tool_calls sequences fold into Explore / Terminal / Changes ToolGroupCards
// (collapsed by default), mixed runs split into multiple groups, switch-off
// kinds pass through as individual cards, and the existing special cards
// (retry-chain errors, subagent spawns, artifact-FileCard carriers) never
// group.
//
// Streaming keeps per-card rendering by ruling R10 — covered by the absence
// of grouping changes in StreamingResponse (untouched here).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'

import { MessageBubble } from '@/components/chat/MessageBubble'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import { clearDiffStatsCache } from '@/components/chat/diffStats'
import { GROUPING_PREF_KEYS } from '@/lib/toolGrouping'
import type { ChatMessage, ToolCall } from '@/types'

vi.mock('@/lib/tauri-api', async importOriginal => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    getFileDiff: vi.fn(async (path: string) => ({
      old_content: `// old ${path}\nconst a = 1\n`,
      new_content: `// new ${path}\nconst a = 2\n`,
      file_name: path.split('/').pop() ?? path,
      language: 'typescript',
    })),
    getTraceTimeline: vi.fn().mockResolvedValue({ session_id: 's', turns: [], cumulative: [] }),
  }
})

const ctx = vi.hoisted(() => ({
  messages: [] as any[],
  sendMessage: vi.fn().mockResolvedValue(true),
  feedback: {} as Record<string, string>,
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  isQuerying: false,
  currentSessionId: 'session-1' as string | null,
  switchSession: vi.fn(),
  refreshSessions: vi.fn().mockResolvedValue(undefined),
  subagentLive: null as any,
  config: null as any,
}))

vi.mock('@/context/ChatContext', () => ({ useChat: () => ctx }))
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return { ...actual, useSessions: () => ctx }
})
vi.mock('@/context/CatalogContext', () => ({ useCatalog: () => ctx }))

function call(tool_name: string, id: string, extra: Partial<ToolCall> = {}): ToolCall {
  return { tool_use_id: id, tool_name, tool_input: {}, status: 'completed', is_error: false, result: 'ok', ...extra }
}

/** Tool-name text nodes NOT inside a group header summary (the summary
 *  echoes the first/last folded names, so it must not read as a rendered
 *  card). */
function cardTexts(name: string): HTMLElement[] {
  return screen
    .queryAllByText(name)
    .filter(el => el.closest('[data-testid="tool-group-summary"]') == null)
}

function bubbleWith(toolCalls: ToolCall[]) {
  const message: ChatMessage = {
    role: 'assistant',
    content: 'Running…',
    timestamp: 1,
    tool_calls: toolCalls,
  }
  return render(
    <I18nProvider>
      <MemoryRouter>
        <ArtifactProvider>
          <MessageBubble message={message} messageIndex={1} onViewDiff={() => {}} regenerate={null} />
        </ArtifactProvider>
      </MemoryRouter>
    </I18nProvider>,
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  clearDiffStatsCache()
  localStorage.clear()
})

describe('MessageBubble — Explore/Terminal/Changes grouping (T11)', () => {
  it('folds a run of consecutive read tools into one collapsed Explore group', () => {
    bubbleWith([call('read_file', 'r1'), call('search', 'r2'), call('web_fetch', 'r3')])
    const group = screen.getByTestId('tool-group-card')
    expect(group).toHaveAttribute('data-group-kind', 'explore')
    expect(screen.getByTestId('tool-group-count')).toHaveTextContent('3 calls')
    expect(screen.getByText('Explore tools')).toBeInTheDocument()
    // Collapsed: the original ToolCallDisplay cards are not in the DOM…
    expect(cardTexts('read_file')).toHaveLength(0)
    // …and expand in place on header click.
    fireEvent.click(screen.getByTestId('tool-group-header'))
    expect(cardTexts('read_file').length).toBeGreaterThan(0)
    expect(cardTexts('search').length).toBeGreaterThan(0)
    expect(cardTexts('web_fetch').length).toBeGreaterThan(0)
    // Exactly one group for one run.
    expect(screen.getAllByTestId('tool-group-card')).toHaveLength(1)
  })

  // Card-less mutating calls (no path in the input → no artifact FileCard)
  // still fold into a Changes group; path-carrying completed writes carry an
  // interactive FileCard and are exempt (case below).
  it('folds consecutive writes into one Changes group', () => {
    bubbleWith([call('write_file', 'w1'), call('edit_file', 'w2')])
    const group = screen.getByTestId('tool-group-card')
    expect(group).toHaveAttribute('data-group-kind', 'changes')
    expect(screen.getByTestId('tool-group-count')).toHaveTextContent('2 calls')
    expect(screen.getByText('File changes')).toBeInTheDocument()
    expect(cardTexts('write_file')).toHaveLength(0)
  })

  it('splits a mixed sequence into one group per consecutive same-kind run plus a lone single', () => {
    bubbleWith([
      call('read_file', 'a'),
      call('search', 'b'),
      call('bash', 'c'),
      call('bash', 'c2'),
      call('write_file', 'd'),
      call('edit_file', 'd2'),
      call('read_file', 'e'),
    ])
    const groups = screen.getAllByTestId('tool-group-card')
    expect(groups).toHaveLength(3)
    expect(groups[0]).toHaveAttribute('data-group-kind', 'explore')
    expect(groups[1]).toHaveAttribute('data-group-kind', 'terminal')
    expect(groups[2]).toHaveAttribute('data-group-kind', 'changes')
    // The trailing single read (run of one) renders as a plain card.
    expect(cardTexts('read_file').length).toBeGreaterThan(0)
    // The folded cards are hidden while their groups stay collapsed.
    expect(cardTexts('search')).toHaveLength(0)
    expect(cardTexts('bash')).toHaveLength(0)
    expect(cardTexts('write_file')).toHaveLength(0)
  })

  it('renders individual cards when the kind\u2019s switch is off (no group)', () => {
    localStorage.setItem(GROUPING_PREF_KEYS.changes, 'false')
    bubbleWith([
      call('write_file', 'w1', { tool_input: { path: '/proj/a.ts' } }),
      call('edit_file', 'w2', { tool_input: { path: '/proj/b.ts' } }),
    ])
    expect(screen.queryByTestId('tool-group-card')).toBeNull()
    expect(cardTexts('write_file').length).toBeGreaterThan(0)
    expect(cardTexts('edit_file').length).toBeGreaterThan(0)
  })

  it('retry-chain errors and subagent spawns keep their special cards and break grouping', () => {
    bubbleWith([
      call('read_file', 'a'),
      call('bash', 'x1', { status: 'error', is_error: true, result: 'boom' }),
      call('bash', 'x2', { status: 'error', is_error: true, result: 'boom again' }),
      call('read_file', 'b'),
    ])
    // The retry-chain banner precedes the two failed cards…
    expect(screen.getByTestId('retry-chain-banner')).toBeInTheDocument()
    // …and NO group forms anywhere: the chain members are non-groupable and
    // every read run is a size-1 single.
    expect(screen.queryByTestId('tool-group-card')).toBeNull()
    expect(cardTexts('bash')).toHaveLength(2)
    expect(cardTexts('read_file')).toHaveLength(2)
  })

  it('orders each banner inline (fix round 1): after the preceding success card, before its chain\u2019s first failed card', () => {
    // Two chains separated by cards and runs — pre-fix, both banners were
    // hoisted to the top of the tool area, breaking the P2-⑨ (D7)
    // failure→retry narrative. DOM order must stay:
    //   read-a, banner-x, x1, x2, read-b, banner-y, y1, y2
    bubbleWith([
      call('read_file', 'a'),
      call('bash', 'x1', { status: 'error', is_error: true, result: 'boom' }),
      call('bash', 'x2', { status: 'error', is_error: true, result: 'boom again' }),
      call('read_file', 'b'),
      call('bash', 'y1', { status: 'error', is_error: true, result: 'boom' }),
      call('bash', 'y2', { status: 'error', is_error: true, result: 'boom' }),
    ])
    const follows = (first: Element, then: Element) =>
      !!(first.compareDocumentPosition(then) & Node.DOCUMENT_POSITION_FOLLOWING)
    const banners = screen.getAllByTestId('retry-chain-banner')
    expect(banners).toHaveLength(2)
    const reads = cardTexts('read_file')
    expect(reads).toHaveLength(2)
    const errs = cardTexts('bash')
    expect(errs).toHaveLength(4)
    // banner-x sits between read-a and its chain's first failed card…
    expect(follows(reads[0], banners[0])).toBe(true)
    expect(follows(banners[0], errs[0])).toBe(true)
    // …does not displace the later content (read-b comes after chain-x)…
    expect(follows(errs[1], reads[1])).toBe(true)
    // …and banner-y stays between read-b and chain-y's first failed card.
    expect(follows(reads[1], banners[1])).toBe(true)
    expect(follows(banners[1], errs[2])).toBe(true)
    expect(screen.queryByTestId('tool-group-card')).toBeNull()
  })

  it('an agent_spawn block is not grouped and its neighbours stay singles', () => {
    bubbleWith([call('read_file', 'a'), call('agent_spawn', 'sub'), call('read_file', 'b')])
    expect(screen.queryByTestId('tool-group-card')).toBeNull()
    expect(screen.getByTestId('subagent-block')).toBeInTheDocument()
    expect(cardTexts('read_file')).toHaveLength(2)
  })

  // CI fix round: a COMPLETED file-mutating call with a path input renders an
  // interactive artifact FileCard (preview / batch run / diff) under its tool
  // block (office Wave 1 A5). T11 folding hid those cards behind the
  // collapsed Changes group — the artifact carriers are exempt, same class as
  // retry-chain members and subagent spawns.
  it('a run of artifact-card writes never folds — every FileCard stays visible', () => {
    // Each call renders as its own plain card with the FileCard attached.
    const view1 = bubbleWith([
      call('write_file', 'f1', { tool_input: { path: '/proj/report.pdf' } }),
      call('write_file', 'f2', { tool_input: { file_path: '/proj/data.csv' } }),
      call('write_file', 'f3', { tool_input: { path: '/proj/notes.md' } }),
    ])
    expect(screen.queryByTestId('tool-group-card')).toBeNull()
    expect(screen.getAllByTestId('file-card')).toHaveLength(3)
    expect(cardTexts('write_file')).toHaveLength(3)
    view1.unmount()

    // The card-carrying run does not block a neighbouring read run from folding.
    bubbleWith([
      call('read_file', 'a'),
      call('search', 'b'),
      call('write_file', 'f1', { tool_input: { path: '/proj/report.pdf' } }),
      call('write_file', 'f2', { tool_input: { file_path: '/proj/data.csv' } }),
    ])
    expect(screen.getAllByTestId('tool-group-card')).toHaveLength(1)
    expect(screen.getByTestId('tool-group-card')).toHaveAttribute('data-group-kind', 'explore')
    expect(screen.getAllByTestId('file-card')).toHaveLength(2)
  })

  it('a failed path-carrying write renders no card and still groups (no artifact to hide)', () => {
    bubbleWith([
      call('edit_file', 'fail1', {
        status: 'error',
        is_error: true,
        result: 'permission denied',
        tool_input: { path: '/proj/a.ts' },
      }),
      call('write_file', 'ok2'),
    ])
    expect(screen.getByTestId('tool-group-card')).toHaveAttribute('data-group-kind', 'changes')
    expect(screen.queryByTestId('file-card')).toBeNull()
  })
})
