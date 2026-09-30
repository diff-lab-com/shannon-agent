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

  // ── R5: rename (inline form — no popups) ──────────────────────────────

  it('rename: a valid new name calls the command and closes the form', async () => {
    const onSwitchedLocal = vi.fn().mockResolvedValue(undefined)
    const renamed = [
      { name: 'default', provider_count: 2, active: true, model: 'claude-sonnet-4-6' },
      { name: 'lab', provider_count: 1, active: false, model: 'gemini-3-pro' },
    ]
    vi.mocked(api.renameProviderProfile).mockResolvedValue(renamed)
    renderSection(onSwitchedLocal)
    await waitFor(() => expect(screen.getByTestId('profile-rename-research')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-rename-research'))
    const input = await screen.findByTestId('profile-rename-input')
    expect(input).toHaveValue('research')
    fireEvent.change(input, { target: { value: 'lab' } })
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
    await waitFor(() => expect(api.renameProviderProfile).toHaveBeenCalledWith('research', 'lab'))
    await waitFor(() => expect(screen.queryByTestId('profile-rename-input')).not.toBeInTheDocument())
    // Rows follow the command's fresh list.
    await waitFor(() => expect(screen.getByTestId('profile-rename-lab')).toBeInTheDocument())
    // Non-active rename → no status/catalog refresh.
    expect(onSwitchedLocal).not.toHaveBeenCalled()
  })

  it('rename: duplicate target is blocked client-side; invalid name never reaches the api', async () => {
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-rename-research')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-rename-research'))
    const input = await screen.findByTestId('profile-rename-input')
    fireEvent.change(input, { target: { value: 'DEFAULT' } })
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('already exists'))
    expect(api.renameProviderProfile).not.toHaveBeenCalled()

    fireEvent.change(input, { target: { value: 'has space' } })
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('whitespace'))
    expect(api.renameProviderProfile).not.toHaveBeenCalled()
  })

  it('rename: unchanged name closes the form without a round trip', async () => {
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-rename-research')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-rename-research'))
    const input = await screen.findByTestId('profile-rename-input')
    fireEvent.change(input, { target: { value: 'research' } })
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
    await waitFor(() => expect(screen.queryByTestId('profile-rename-input')).not.toBeInTheDocument())
    expect(api.renameProviderProfile).not.toHaveBeenCalled()
  })

  it('rename: engine duplicate error surfaces via the error toast path, form stays open', async () => {
    vi.mocked(api.renameProviderProfile).mockRejectedValue(new Error("A profile named 'x' already exists"))
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-rename-research')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-rename-research'))
    const input = await screen.findByTestId('profile-rename-input')
    fireEvent.change(input, { target: { value: 'lab' } })
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
    await waitFor(() => expect(api.renameProviderProfile).toHaveBeenCalledWith('research', 'lab'))
    // The form stays open (the user can correct the name).
    await waitFor(() => expect(screen.getByTestId('profile-rename-input')).toBeInTheDocument())
  })

  it('rename of the ACTIVE profile refreshes status + catalog', async () => {
    const onSwitched = vi.fn().mockResolvedValue(undefined)
    const renamed = [
      { name: 'main', provider_count: 2, active: true, model: 'claude-sonnet-4-6' },
      { name: 'research', provider_count: 1, active: false, model: 'gemini-3-pro' },
    ]
    vi.mocked(api.renameProviderProfile).mockResolvedValue(renamed)
    renderSection(onSwitched)
    await waitFor(() => expect(screen.getByTestId('profile-rename-default')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-rename-default'))
    const input = await screen.findByTestId('profile-rename-input')
    fireEvent.change(input, { target: { value: 'main' } })
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
    await waitFor(() => expect(api.renameProviderProfile).toHaveBeenCalledWith('default', 'main'))
    await waitFor(() => expect(onSwitched).toHaveBeenCalledTimes(1))
  })

  // ── R5: delete (ConfirmDialog — jsdom-safe, per the established pattern) ─

  it('delete: cancel is a no-op', async () => {
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-delete-research')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-delete-research'))
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog.textContent).toContain('research')
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(api.deleteProviderProfile).not.toHaveBeenCalled()
  })

  it('delete: confirming calls the command with force and swaps the rows', async () => {
    const remaining = [{ name: 'default', provider_count: 2, active: true, model: 'claude-sonnet-4-6' }]
    vi.mocked(api.deleteProviderProfile).mockResolvedValue({
      profiles: remaining,
      became_active: null,
    })
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-delete-research')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-delete-research'))
    await waitFor(() => expect(screen.getByRole('alertdialog')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Delete profile' }))
    await waitFor(() => expect(api.deleteProviderProfile).toHaveBeenCalledWith('research', true))
    await waitFor(() => expect(screen.getAllByTestId('profile-row')).toHaveLength(1))
    // Inactive delete → no pointer move → no refresh.
    expect(api.setActiveProviderProfile).not.toHaveBeenCalled()
  })

  it('delete: the ACTIVE profile warning names the engine fallback', async () => {
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-delete-default')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-delete-default'))
    const dialog = await screen.findByRole('alertdialog')
    // Client mirror of remove_model_profile's fallback rule: default
    // survives → 'default' is named as the takeover.
    expect(dialog.textContent).toContain('default')
    expect(dialog.textContent).toContain('becomes active')
    expect(api.deleteProviderProfile).not.toHaveBeenCalled()
  })

  it('delete: removing the ACTIVE profile triggers the status refresh with the fallback', async () => {
    const onSwitched = vi.fn().mockResolvedValue(undefined)
    const remaining = [{ name: 'default', provider_count: 2, active: true, model: 'claude-sonnet-4-6' }]
    vi.mocked(api.deleteProviderProfile).mockResolvedValue({
      profiles: remaining,
      became_active: 'default',
    })
    renderSection(onSwitched)
    await waitFor(() => expect(screen.getByTestId('profile-delete-default')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('profile-delete-default'))
    await waitFor(() => expect(screen.getByRole('alertdialog')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Delete profile' }))
    await waitFor(() => expect(api.deleteProviderProfile).toHaveBeenCalledWith('default', true))
    await waitFor(() => expect(onSwitched).toHaveBeenCalledTimes(1))
  })

  it('delete: the last remaining profile cannot be deleted (affordance disabled)', async () => {
    seed([{ name: 'default', provider_count: 2, active: true, model: 'claude-sonnet-4-6' }])
    renderSection()
    await waitFor(() => expect(screen.getByTestId('profile-delete-default')).toBeInTheDocument())
    expect(screen.getByTestId('profile-delete-default')).toBeDisabled()
  })
})
