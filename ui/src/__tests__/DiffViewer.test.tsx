import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import DiffViewer from '@/components/diff/DiffViewer'
import type { FileDiff } from '@/types'

const baseDiff: FileDiff = {
  old_content: 'line one\nline two\nline three',
  new_content: 'line one\nline two edited\nline three\nline four',
  file_name: 'src/app.ts',
  language: 'typescript',
}

describe('DiffViewer', () => {
  it('renders file name, language, and change counts', () => {
    render(<DiffViewer diff={baseDiff} />)
    expect(screen.getByText('src/app.ts')).toBeInTheDocument()
    expect(screen.getByText(/typescript/i)).toBeInTheDocument()
    expect(screen.getByText('+2')).toBeInTheDocument()
    expect(screen.getByText('−1')).toBeInTheDocument()
  })

  it('renders added and deleted line content', () => {
    render(<DiffViewer diff={baseDiff} />)
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
    render(<DiffViewer diff={same} />)
    expect(screen.getByText('No changes.')).toBeInTheDocument()
  })

  it('handles pure addition (new file)', () => {
    const added: FileDiff = {
      old_content: '',
      new_content: 'fresh\ncontent',
      file_name: 'new.ts',
      language: 'typescript',
    }
    render(<DiffViewer diff={added} />)
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
    render(<DiffViewer diff={removed} />)
    expect(screen.getByText('gone')).toBeInTheDocument()
    expect(screen.getByText('content')).toBeInTheDocument()
    expect(screen.getByText('−2')).toBeInTheDocument()
  })
})
