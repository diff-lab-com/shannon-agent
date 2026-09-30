// R3-2 (desktop slice) — Profiles section: state-logic wiring tests.
// Plain button/form events only (native elements + the jsdom-safe
// ConfirmDialog) — no Base-UI popup interactions, per the batch brief.

import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { ProfilesSection } from '@/components/settings/models-settings/ProfilesSection'
import * as api from '@/lib/tauri-api'
import type { ProviderProfileSummary } from '@/types'

const rowsStore: { current: ProviderProfileSummary[] } = { current: [] }

function seed(rows: ProviderProfileSummary[]) {
  rowsStore.current = rows
  vi.mocked(api.listProviderProfiles).mockResolvedValue(rows)
}

const DEFAULT_ROWS: ProviderProfileSummary[] = [
  { name: 'default', provider_count: 2, active: true, model: 'claude-sonnet-4-6' },
  { name: 'research', provider_count: 1, active: false, model: 'gemini-3-pro' },
]

function renderSection(onSwitched?: () => Promise<void>) {
  return render(<ProfilesSection onSwitched={onSwitched} />)
}

describe('ProfilesSection (R3-2)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    seed(DEFAULT_ROWS)
    vi.mocked(api.createProviderProfile).mockResolvedValue(DEFAULT_ROWS)
    vi.mocked(api.setActiveProviderProfile).mockResolvedValue(DEFAULT_ROWS)
  })

  it('renders the roster with the active marker and provider counts', async () => {
    renderSection()
    await waitFor(() => expect(screen.getAllByTestId('profile-row')).toHaveLength(2))
    const active = screen.getByText('Active')
    expect(active).toBeInTheDocument()
    // The active row has NO switch button; the inactive one does.
    expect(screen.queryByTestId('profile-switch-default')).toBeNull()
    expect(screen.getByTestId('profile-switch-research')).not.toBeNull()
  })

  it('switches directly when the target has providers, refreshing status+catalog', async () => {
    const onSwitched = vi.fn().mockResolvedValue(undefined)
    const switched = [
      { name: 'default', provider_count: 2, active: false, model: 'claude-sonnet-4-6' },
      { name: 'research', provider_count: 1, active: true, model: 'gemini-3-pro' },
    ]
    vi.mocked(api.setActiveProviderProfile).mockResolvedValue(switched)
    renderSection(onSwitched)
    await waitFor(() => expect(screen.getByTestId('profile-switch-research')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-switch-research'))
    await waitFor(() =>
      expect(api.setActiveProviderProfile).toHaveBeenCalledWith('research'),
    )
    await waitFor(() => expect(onSwitched).toHaveBeenCalledTimes(1))
    // Rows follow the command's fresh list — research is now the active one.
    await waitFor(() => expect(screen.queryByTestId('profile-switch-research')).toBeNull())
  })

  it('asks for confirmation before switching to an EMPTY profile; cancel is a no-op', async () => {
    seed([
      { name: 'default', provider_count: 2, active: true, model: null },
      { name: 'scratch', provider_count: 0, active: false, model: null },
    ])
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-switch-scratch')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-switch-scratch'))
    // Confirm dialog appears naming the profile; nothing fired yet.
    await waitFor(() => expect(screen.getByRole('alertdialog')).toBeInTheDocument())
    expect(screen.getByRole('alertdialog').textContent).toContain('scratch')
    expect(api.setActiveProviderProfile).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(api.setActiveProviderProfile).not.toHaveBeenCalled()
  })

  it('confirming the empty-profile dialog performs the switch', async () => {
    seed([
      { name: 'default', provider_count: 2, active: true, model: null },
      { name: 'scratch', provider_count: 0, active: false, model: null },
    ])
    const switched = [
      { name: 'default', provider_count: 2, active: false, model: null },
      { name: 'scratch', provider_count: 0, active: true, model: null },
    ]
    vi.mocked(api.setActiveProviderProfile).mockResolvedValue(switched)
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-switch-scratch')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-switch-scratch'))
    await waitFor(() => expect(screen.getByRole('alertdialog')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Switch anyway' }))
    await waitFor(() =>
      expect(api.setActiveProviderProfile).toHaveBeenCalledWith('scratch'),
    )
  })

  it('create: invalid names are rejected client-side without an api call', async () => {
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-create')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-create'))
    const input = await screen.findByTestId('profile-name-input')
    fireEvent.change(input, { target: { value: 'my profile' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toContain('whitespace'),
    )
    expect(api.createProviderProfile).not.toHaveBeenCalled()
  })

  it('create: a valid name calls the command and closes the form', async () => {
    const withNew = [...DEFAULT_ROWS, { name: 'work', provider_count: 0, active: false, model: null }]
    vi.mocked(api.createProviderProfile).mockResolvedValue(withNew)
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-create')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-create'))
    const input = await screen.findByTestId('profile-name-input')
    fireEvent.change(input, { target: { value: 'work' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    await waitFor(() => expect(api.createProviderProfile).toHaveBeenCalledWith('work'))
    await waitFor(() => expect(screen.queryByTestId('profile-name-input')).not.toBeInTheDocument())
    await waitFor(() => expect(screen.getByTestId('profile-switch-work')).toBeInTheDocument())
  })

  it('duplicate names are blocked before the api call', async () => {
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-create')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-create'))
    const input = await screen.findByTestId('profile-name-input')
    fireEvent.change(input, { target: { value: 'DEFAULT' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    await waitFor(() => expect(screen.getByRole('alert')).toBeInTheDocument())
    expect(api.createProviderProfile).not.toHaveBeenCalled()
  })
})
