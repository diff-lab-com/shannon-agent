// Batch C (2026-09-20 delta analysis §批C): chat-surface artifact & status.
//  - C1: the artifact chip renders as a card — icon tile, display title,
//    localized kind badge, explicit 打开 affordance
//  - C2: diff line-stats computation over getFileDiff payloads
//  - C3: the run status pill shows elapsed time and the active tool

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { ArtifactChip } from '@/components/artifact/ArtifactChip'
import { RunStatusLine } from '@/pages/chat/MessageArea'
import { fetchDiffLineStats } from '@/components/chat/diffStats'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'

vi.mock('@/lib/tauri-api', async importOriginal => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    getFileDiff: vi.fn(async (path: string) => {
      if (path === '/p/added.ts') {
        return { path, old_content: 'a\nb\nc\n', new_content: 'a\nB\nc\nd\ne\n' }
      }
      return { path, old_content: 'x\ny\n', new_content: 'x\ny\n' }
    }),
  }
})

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <MemoryRouter>
        <ArtifactProvider>{ui}</ArtifactProvider>
      </MemoryRouter>
    </I18nProvider>
  )
}

describe('artifact card (C1)', () => {
  const doc = { kind: 'document' as const, source: '# 报告\n…', title: '调研报告', confidence: 'medium' as const }

  it('renders title, localized kind badge and the open affordance', () => {
    render(wrap(<ArtifactChip artifact={doc} />))
    expect(screen.getByTestId('artifact-card')).toBeInTheDocument()
    expect(screen.getByText('调研报告')).toBeInTheDocument()
    expect(screen.getByText('Document')).toBeInTheDocument() // en locale kind badge
    expect(screen.getByText('Open')).toBeInTheDocument() // en 打开 affordance
  })

  it('falls back to the localized title when detection found none', () => {
    render(wrap(<ArtifactChip artifact={{ kind: 'svg', source: '<svg/>', title: '', confidence: 'high' }} />))
    expect(screen.getByText('SVG graphic')).toBeInTheDocument()
  })
})

describe('diff line stats (C2)', () => {
  beforeEach(() => vi.clearAllMocks())

  it('counts added/removed lines client-side', async () => {
    const stats = await fetchDiffLineStats('/p/added.ts')
    expect(stats).toEqual({ additions: 3, deletions: 1 })
  })

  it('caches per path (one IPC per file)', async () => {
    const api = await import('@/lib/tauri-api')
    const mocked = vi.mocked(api.getFileDiff)
    // Fresh path: the stats cache is module-level and prior tests may have
    // already warmed other paths.
    const path = `/p/cache-${Math.random().toString(36).slice(2)}.ts`
    mocked.mockClear()
    await fetchDiffLineStats(path)
    await fetchDiffLineStats(path)
    expect(mocked).toHaveBeenCalledTimes(1)
  })

  it('returns null stats for identical content without throwing', async () => {
    const stats = await fetchDiffLineStats('/p/same.ts')
    expect(stats).toEqual({ additions: 0, deletions: 0 })
  })
})

describe('run status line (C3)', () => {
  it('shows elapsed time and the active tool while live', () => {
    render(wrap(<RunStatusLine startedAt={Date.now() - 65_000} activeTool="bash" />))
    const pill = screen.getByTestId('run-status-line')
    expect(pill).toHaveTextContent('Worked 1m')
    expect(pill).toHaveTextContent('Running bash')
  })

  it('omits the tool clause when no tool is active', () => {
    render(wrap(<RunStatusLine startedAt={Date.now() - 10_000} activeTool={null} />))
    const pill = screen.getByTestId('run-status-line')
    expect(pill).toHaveTextContent('Worked')
    expect(pill).not.toHaveTextContent('Running')
  })

  it('renders without a start time (run began before this window joined)', async () => {
    render(wrap(<RunStatusLine startedAt={null} activeTool={null} />))
    await waitFor(() => expect(screen.getByTestId('run-status-line')).toBeInTheDocument())
  })
})

// P2-19: QUERY_TOOL_PROGRESS surfaces on the pill as a `· 45%` chip next to
// the tool name plus the backend's progress_message (truncated, full text in
// title). Absent progress renders exactly as before (the tests above).
describe('run status line tool progress (P2-19)', () => {
  it('shows a percentage chip and the progress message', () => {
    render(wrap(
      <RunStatusLine
        startedAt={Date.now() - 10_000}
        activeTool="bash"
        toolProgress={{ progress: 45.4, message: 'Running unit tests' }}
      />,
    ))
    const pill = screen.getByTestId('run-status-line')
    expect(pill).toHaveTextContent('Running bash')
    expect(screen.getByTestId('run-progress-pct')).toHaveTextContent('· 45%')
    const msg = screen.getByTestId('run-progress-message')
    expect(msg).toHaveTextContent('Running unit tests')
    expect(msg).toHaveAttribute('title', 'Running unit tests')
  })

  it('renders the message without a percentage when only text arrives', () => {
    render(wrap(
      <RunStatusLine
        startedAt={null}
        activeTool="edit_file"
        toolProgress={{ message: 'Rewriting src/main.rs' }}
      />,
    ))
    expect(screen.queryByTestId('run-progress-pct')).not.toBeInTheDocument()
    expect(screen.getByTestId('run-progress-message')).toHaveTextContent('Rewriting src/main.rs')
  })

  it('ignores an out-of-range percentage', () => {
    render(wrap(
      <RunStatusLine
        startedAt={null}
        activeTool="bash"
        toolProgress={{ progress: 120, message: 'x' }}
      />,
    ))
    expect(screen.queryByTestId('run-progress-pct')).not.toBeInTheDocument()
  })

  it('ignores a non-finite percentage', () => {
    render(wrap(
      <RunStatusLine
        startedAt={null}
        activeTool="bash"
        toolProgress={{ progress: Number.NaN, message: 'x' }}
      />,
    ))
    expect(screen.queryByTestId('run-progress-pct')).not.toBeInTheDocument()
  })

  it('caps the title attribute at 200 chars while the label truncates', () => {
    const long = 'x'.repeat(300)
    render(wrap(
      <RunStatusLine
        startedAt={null}
        activeTool="bash"
        toolProgress={{ progress: 10, message: long }}
      />,
    ))
    const msg = screen.getByTestId('run-progress-message')
    expect(msg).toHaveAttribute('title', 'x'.repeat(200))
  })

  it('renders exactly as before when no progress is present', () => {
    render(wrap(<RunStatusLine startedAt={Date.now() - 5_000} activeTool="bash" toolProgress={null} />))
    expect(screen.queryByTestId('run-progress-pct')).not.toBeInTheDocument()
    expect(screen.queryByTestId('run-progress-message')).not.toBeInTheDocument()
    expect(screen.getByTestId('run-status-line')).toHaveTextContent('Running bash')
  })
})
