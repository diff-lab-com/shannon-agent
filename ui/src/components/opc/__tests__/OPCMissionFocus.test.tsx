import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import OPCMissionFocus from '@/components/opc/OPCMissionFocus'

vi.mock('@/lib/tauri-api', () => ({
  configure: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

function renderFocus(config: { provider?: string; strategic_focus?: string } | null = null) {
  return render(
    <MemoryRouter>
      <OPCMissionFocus config={config} />
    </MemoryRouter>
  )
}

describe('OPCMissionFocus', () => {
  it('renders default heading without config', () => {
    renderFocus(null)
    expect(screen.getByText(/Autonomous task execution through multi-agent/)).toBeInTheDocument()
  })

  it('renders provider-based heading with config', () => {
    renderFocus({ provider: 'anthropic' })
    expect(screen.getByText(/Anthropic Agent Orchestration/)).toBeInTheDocument()
  })

  it('shows today\'s mission label', () => {
    renderFocus()
    expect(screen.getByText("Today's Mission")).toBeInTheDocument()
  })

  it('shows Edit button', () => {
    renderFocus()
    expect(screen.getByText('Edit')).toBeInTheDocument()
  })

  it('toggles to textarea on Edit click', () => {
    renderFocus()
    fireEvent.click(screen.getByText('Edit'))
    expect(screen.getByText('Save Focus')).toBeInTheDocument()
    expect(screen.getByLabelText("Edit today's mission")).toBeInTheDocument()
  })

  it('reverts to display mode on Cancel', () => {
    renderFocus()
    fireEvent.click(screen.getByText('Edit'))
    expect(screen.getByText('Cancel')).toBeInTheDocument()
    fireEvent.click(screen.getByText('Cancel'))
    expect(screen.getByText('Edit')).toBeInTheDocument()
  })

  it('shows custom strategic_focus when set', () => {
    renderFocus({ strategic_focus: 'Ship the MVP by Q3.' })
    expect(screen.getByText('Ship the MVP by Q3.')).toBeInTheDocument()
  })

  it('has aria-expanded reflecting edit state', () => {
    renderFocus()
    const btn = screen.getByRole('button', { name: 'Edit' })
    expect(btn).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(btn)
    expect(btn).toHaveAttribute('aria-expanded', 'true')
  })
})
