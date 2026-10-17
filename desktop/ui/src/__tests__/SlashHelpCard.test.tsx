// H2 P1-3 — the /help roster card. The card must list ONLY what really
// exists: every static command straight off SLASH_COMMANDS, plus installed
// skills from the best-effort fetchSlashSkills — a failed or empty skill
// fetch degrades to the commands-only roster (honesty contract).

import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import SlashResultCard from '@/components/chat/SlashResultCard'
import { SLASH_COMMANDS } from '@/lib/slash/commands'
import * as api from '@/lib/tauri-api'

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return { ...actual, listSkills: vi.fn() }
})

function renderHelp() {
  return render(<SlashResultCard result={{ kind: 'help' }} onDismiss={() => {}} />)
}

describe('SlashResultCard — /help roster', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('lists every static command with its description', async () => {
    vi.mocked(api.listSkills).mockResolvedValue([])
    renderHelp()

    expect(screen.getByText('Slash commands')).toBeInTheDocument()
    const roster = screen.getByTestId('slash-help-roster')
    for (const cmd of SLASH_COMMANDS) {
      expect(roster.textContent).toContain(`/${cmd.name}`)
    }
    // Spot-check a description resolves from the real message table (i18n
    // provider is auto-applied by the test setup).
    expect(screen.getByText('Open Settings')).toBeInTheDocument()
    // No skills installed → the skills section never renders.
    expect(screen.queryByText('Installed skills')).not.toBeInTheDocument()
  })

  it('lists installed skills after the commands', async () => {
    vi.mocked(api.listSkills).mockResolvedValue([
      { name: 'pdf-reader', description: 'Reads PDFs', trigger: '/pdf-reader', source: 'User', category: null },
      { name: 'code-review', description: 'Reviews code', trigger: '/code-review', source: 'Project', category: null },
    ])
    renderHelp()

    await waitFor(() => expect(screen.getByText('Installed skills')).toBeInTheDocument())
    const roster = screen.getByTestId('slash-help-roster')
    expect(roster.textContent).toContain('/pdf-reader')
    expect(roster.textContent).toContain('/code-review')
    // The trailing note about skill send semantics is present.
    expect(roster.textContent).toContain('/name args')
  })

  it('degrades to commands-only when the skill fetch fails', async () => {
    vi.mocked(api.listSkills).mockRejectedValue(new Error('backend down'))
    renderHelp()

    // Commands render regardless (fetchSlashSkills swallows the rejection).
    await waitFor(() => expect(screen.getByTestId('slash-help-roster')).toBeInTheDocument())
    expect(screen.getByText('Open Settings')).toBeInTheDocument()
    // Let any pending state settle, then confirm the section stayed hidden.
    await waitFor(() => expect(vi.mocked(api.listSkills)).toHaveBeenCalled())
    expect(screen.queryByText('Installed skills')).not.toBeInTheDocument()
  })

  it('renders through the shared card chrome (dismissable status card)', () => {
    vi.mocked(api.listSkills).mockResolvedValue([])
    renderHelp()
    expect(screen.getByRole('status')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeInTheDocument()
  })
})
