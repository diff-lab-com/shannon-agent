import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { ResearchReportModal } from '@/components/chat/ResearchReportModal'
import type { ResearchReport } from '@/types'

const sampleReport: ResearchReport = {
  title: 'Vector DB Comparison',
  summary: 'A short comparison of pgvector, Qdrant, and Weaviate.',
  sections: [
    {
      heading: 'Latency',
      body: 'pgvector matches Qdrant within 5% on HNSW queries [1]. Weaviate trails by ~12% [2].',
    },
  ],
  citations: [
    { id: 1, title: 'pgvector benchmarks', url: 'https://example.com/pg', source: 'github.com' },
    { id: 2, title: 'Weaviate audit', url: 'https://example.com/w', source: 'weaviate.io' },
  ],
  generated_at: 1717000000000,
}

describe('ResearchReportModal', () => {
  it('renders nothing when closed', () => {
    const { container } = render(
      <ResearchReportModal report={sampleReport} open={false} onClose={() => {}} />,
    )
    expect(container.firstChild).toBeNull()
  })

  it('renders title, summary, sections, and citations when open', async () => {
    render(<ResearchReportModal report={sampleReport} open onClose={() => {}} />)
    expect(await screen.findByText('Vector DB Comparison')).toBeInTheDocument()
    expect(screen.getByText('A short comparison of pgvector, Qdrant, and Weaviate.')).toBeInTheDocument()
    expect(screen.getByText('Latency')).toBeInTheDocument()
    expect(screen.getByText('pgvector matches Qdrant within 5% on HNSW queries')).toBeInTheDocument()
    expect(screen.getByText('pgvector benchmarks')).toBeInTheDocument()
    expect(screen.getByText('Weaviate audit')).toBeInTheDocument()
  })

  it('renders citation number badges for [N] refs in section body', () => {
    render(<ResearchReportModal report={sampleReport} open onClose={() => {}} />)
    const badges = screen.getAllByLabelText('Citation 1')
    expect(badges.length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByLabelText('Citation 2').length).toBeGreaterThanOrEqual(1)
  })

  it('highlights the matching citation when a [N] badge is clicked', async () => {
    render(<ResearchReportModal report={sampleReport} open onClose={() => {}} />)
    const badge = screen.getAllByLabelText('Citation 2')[0]
    fireEvent.click(badge)
    await waitFor(() => {
      const el = document.querySelector('[data-citation-id="2"]')
      expect(el?.classList.contains('border-primary/50')).toBe(true)
    })
  })

  it('calls onClose when the close button is clicked', () => {
    const onClose = vi.fn()
    render(<ResearchReportModal report={sampleReport} open onClose={onClose} />)
    const closeBtn = screen.getByLabelText('Close report')
    fireEvent.click(closeBtn)
    expect(onClose).toHaveBeenCalled()
  })

  it('shows empty state when no citations', () => {
    const noCitations: ResearchReport = {
      ...sampleReport,
      citations: [],
      sections: [{ heading: 'Note', body: 'No sources needed.' }],
    }
    render(<ResearchReportModal report={noCitations} open onClose={() => {}} />)
    expect(screen.getByText('No citations attached.')).toBeInTheDocument()
  })
})
