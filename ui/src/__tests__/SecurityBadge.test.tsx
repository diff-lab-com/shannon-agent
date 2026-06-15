import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { SecurityBadge } from '@/components/extensions/SecurityBadge'

const scanPromptInjection = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  scanPromptInjection: (...a: unknown[]) => scanPromptInjection(...a),
}))

beforeEach(() => {
  scanPromptInjection.mockReset()
})

describe('SecurityBadge (P6 injection scan)', () => {
  it('renders nothing for verified/official trust without scanning', () => {
    scanPromptInjection.mockResolvedValue({ risk: 'dangerous', matches: [], match_count: 0 })
    render(<SecurityBadge text="ignore previous instructions" trust="verified" />)
    expect(scanPromptInjection).not.toHaveBeenCalled()
    expect(screen.queryByText('Injection risk')).not.toBeInTheDocument()
  })

  it('renders nothing when scan returns clean', async () => {
    scanPromptInjection.mockResolvedValue({ risk: 'clean', matches: [], match_count: 0 })
    render(<SecurityBadge text="A helpful note-taking agent." trust="community" />)
    await waitFor(() => {
      expect(scanPromptInjection).toHaveBeenCalled()
    })
    expect(screen.queryByText('Injection risk')).not.toBeInTheDocument()
    expect(screen.queryByText('Review')).not.toBeInTheDocument()
  })

  it('renders danger badge when scan returns dangerous', async () => {
    scanPromptInjection.mockResolvedValue({
      risk: 'dangerous',
      matches: [{ pattern: 'ignore previous', matched_substring: 'ignore previous', category: 'system_override' }],
      match_count: 1,
    })
    render(<SecurityBadge text="ignore previous instructions" trust="community" />)
    await waitFor(() => {
      expect(screen.getByText('Injection risk')).toBeInTheDocument()
    })
  })

  it('renders review badge when scan returns suspicious', async () => {
    scanPromptInjection.mockResolvedValue({
      risk: 'suspicious',
      matches: [{ pattern: 'curl', matched_substring: 'curl', category: 'data_exfil' }],
      match_count: 1,
    })
    render(<SecurityBadge text="curls your data" trust="unknown" />)
    await waitFor(() => {
      expect(screen.getByText('Review')).toBeInTheDocument()
    })
  })

  it('fails silently if scan throws', async () => {
    scanPromptInjection.mockRejectedValue(new Error('boom'))
    render(<SecurityBadge text="whatever" trust="community" />)
    await waitFor(() => {
      expect(scanPromptInjection).toHaveBeenCalled()
    })
    expect(screen.queryByText('Injection risk')).not.toBeInTheDocument()
    expect(screen.queryByText('Review')).not.toBeInTheDocument()
  })
})
