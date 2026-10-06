import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { DocumentsSkillsList } from '../DocumentsSkillsList'

// Per-file mock (hoisted factory): this component only needs the probe —
// global setup mocks the whole module, but we need controllable returns.
vi.mock('@/lib/tauri-api', () => ({
  probeHostRuntime: vi.fn(),
}))

import { probeHostRuntime } from '@/lib/tauri-api'

const mockProbe = vi.mocked(probeHostRuntime)

describe('DocumentsSkillsList (Office Wave 1 A3\')', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('lists built-in document commands and no install buttons when python3 is present', async () => {
    mockProbe.mockResolvedValue({ python3: true, pythonVersion: 'Python 3.12.3', pandoc: false, libreoffice: false })
    const { container } = render(<DocumentsSkillsList />)
    await waitFor(() => {
      expect(screen.getByText('/docx-report')).toBeInTheDocument()
    })
    expect(screen.getByText('/xlsx-table')).toBeInTheDocument()
    expect(screen.getByText('/ppt-outline')).toBeInTheDocument()
    // Honest v1: the community repos are unpublished — no install affordances.
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
    expect(container.textContent).not.toContain('welcome.skills.install')
  })

  it('shows the Python-missing hint when the host lacks python3', async () => {
    mockProbe.mockResolvedValue({ python3: false, pythonVersion: null, pandoc: false, libreoffice: false })
    render(<DocumentsSkillsList />)
    await waitFor(() => {
      expect(screen.getByText('Python 3 not detected')).toBeInTheDocument()
    })
    expect(screen.queryByText('/docx-report')).not.toBeInTheDocument()
  })

  it('renders nothing while the probe is unresolved', () => {
    mockProbe.mockReturnValue(new Promise(() => {}))
    const { container } = render(<DocumentsSkillsList />)
    expect(container).toBeEmptyDOMElement()
  })

  it('renders nothing when the probe fails or returns garbage (no false claims)', async () => {
    mockProbe.mockRejectedValue(new Error('boom'))
    const { container } = render(<DocumentsSkillsList />)
    await waitFor(() => {
      expect(container).toBeEmptyDOMElement()
    })
  })
})
