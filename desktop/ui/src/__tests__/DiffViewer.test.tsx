import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import DiffViewer from '@/components/diff/DiffViewer'
import { computeHunks } from '@/lib/diff-merge'
import type { FileDiff } from '@/types'

const baseDiff: FileDiff = {
  old_content: 'line one\nline two\nline three',
  new_content: 'line one\nline two edited\nline three\nline four',
  file_name: 'src/app.ts',
  language: 'typescript',
}

describe('DiffViewer', () => {
  it('renders file name, language, and change counts', () => {
    render(<DiffViewer diff={baseDiff} decisions={new Map()} />)
    expect(screen.getByText('src/app.ts')).toBeInTheDocument()
    expect(screen.getByText(/typescript/i)).toBeInTheDocument()
    expect(screen.getByText('+2')).toBeInTheDocument()
    expect(screen.getByText('−1')).toBeInTheDocument()
  })

  it('renders added and deleted line content', () => {
    render(<DiffViewer diff={baseDiff} decisions={new Map()} />)
    expect(screen.getByText('line two')).toBeInTheDocument()
    expect(screen.getByText('line two edited')).toBeInTheDocument()
    expect(screen.getByText('line four')).toBeInTheDocument()
  })

  it('shows "No changes" when contents are identical', () => {
    const same: FileDiff = {
      old_content: 'a\nb',
      new_content: 'a\nb',
      file_name: 'noop.txt',
      language: 'text',
    }
    render(<DiffViewer diff={same} decisions={new Map()} />)
    expect(screen.getByText('No changes.')).toBeInTheDocument()
  })

  it('handles pure addition (new file)', () => {
    const added: FileDiff = {
      old_content: '',
      new_content: 'fresh\ncontent',
      file_name: 'new.ts',
      language: 'typescript',
    }
    render(<DiffViewer diff={added} decisions={new Map()} />)
    expect(screen.getByText('fresh')).toBeInTheDocument()
    expect(screen.getByText('content')).toBeInTheDocument()
    expect(screen.getByText('+2')).toBeInTheDocument()
  })

  it('handles pure deletion', () => {
    const removed: FileDiff = {
      old_content: 'gone\ncontent',
      new_content: '',
      file_name: 'deleted.ts',
      language: 'typescript',
    }
    render(<DiffViewer diff={removed} decisions={new Map()} />)
    expect(screen.getByText('gone')).toBeInTheDocument()
    expect(screen.getByText('content')).toBeInTheDocument()
    expect(screen.getByText('−2')).toBeInTheDocument()
  })
})

describe('DiffViewer — per-hunk controls (Day 3)', () => {
  it('renders a hunk header with Undecided state by default', () => {
    // baseDiff produces 2 hunks (line-2 replacement + line-4 pure insertion).
    render(<DiffViewer diff={baseDiff} decisions={new Map()} />)
    expect(screen.getAllByText('Undecided')).toHaveLength(2)
  })

  it('shows Accepted state when the hunk is accepted', () => {
    // baseDiff produces one hunk; its id is deterministic but unknown here.
    // Easiest: compute it via the same lib the component uses.
    // computeHunks imported at top
    const hunks = computeHunks(baseDiff.old_content, baseDiff.new_content)
    const decisions = new Map([[hunks[0].id, 'accept']])
    render(<DiffViewer diff={baseDiff} decisions={decisions} />)
    expect(screen.getByText('Accepted')).toBeInTheDocument()
  })

  it('shows Rejected state when the hunk is rejected', () => {
    // computeHunks imported at top
    const hunks = computeHunks(baseDiff.old_content, baseDiff.new_content)
    const decisions = new Map([[hunks[0].id, 'reject']])
    render(<DiffViewer diff={baseDiff} decisions={decisions} />)
    expect(screen.getByText('Rejected')).toBeInTheDocument()
  })

  it('calls onToggleHunk with the first hunk id when its header is clicked', () => {
    // computeHunks imported at top
    const hunks = computeHunks(baseDiff.old_content, baseDiff.new_content)
    const onToggle = vi.fn()
    render(<DiffViewer diff={baseDiff} decisions={new Map()} onToggleHunk={onToggle} />)
    const headerBtns = screen.getAllByRole('button', { name: /Toggle hunk decision/i })
    fireEvent.click(headerBtns[0])
    expect(onToggle).toHaveBeenCalledWith(hunks[0].id)
  })

  it('disables all header buttons when onToggleHunk is not supplied', () => {
    render(<DiffViewer diff={baseDiff} decisions={new Map()} />)
    const headerBtns = screen.getAllByRole('button', { name: /Toggle hunk decision/i })
    expect(headerBtns).toHaveLength(2)
    for (const btn of headerBtns) {
      expect(btn).toBeDisabled()
    }
  })

  it('renders two hunk headers for non-adjacent changes', () => {
    const diff: FileDiff = {
      old_content: 'a\nb\nc\nd\ne',
      new_content: 'A\nb\nc\nD\ne',
      file_name: 'multi.txt',
      language: 'text',
    }
    render(<DiffViewer diff={diff} decisions={new Map()} />)
    // Two "Undecided" state pills, one per hunk.
    expect(screen.getAllByText('Undecided')).toHaveLength(2)
  })

  it('renders a per-hunk line-count label', () => {
    render(<DiffViewer diff={baseDiff} decisions={new Map()} />)
    // baseDiff hunk has 1 del + 2 adds = 3 lines, plus the trailing context
    // pair baked into the same hunk by diffLines. The exact count is not
    // asserted here — just that some "N lines" label exists.
    expect(screen.getAllByText(/lines$/).length).toBeGreaterThan(0)
  })
})
