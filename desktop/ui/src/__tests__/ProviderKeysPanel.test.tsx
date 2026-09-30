// R4-3 (desktop slice) — API-keys panel: list/activate/remove/add state
// logic through plain buttons + the jsdom-safe ConfirmDialog (the same
// posture the ProfilesSection tests use — no Base-UI popup interactions).

import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { ProviderKeysPanel } from '@/components/settings/models-settings/ProviderKeysPanel'
import * as api from '@/lib/tauri-api'
import type { ProviderConnection, ProviderKeySummary } from '@/types'

const CONN: ProviderConnection = {
  id: 'prov-anthropic',
  display_name: 'Anthropic',
  kind: 'anthropic',
  has_api_key: true,
  base_url: null,
}

const ROTATION: ProviderKeySummary[] = [
  { index: 0, active: true, masked_hint: 'sk-ant…aaaa' },
  { index: 1, active: false, masked_hint: 'sk-ant…bbbb' },
]

function renderPanel(onKeysChanged?: () => Promise<void>) {
  return render(<ProviderKeysPanel conn={CONN} onClose={() => {}} onKeysChanged={onKeysChanged} />)
}

describe('ProviderKeysPanel (R4-3 desktop)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.listProviderKeys).mockResolvedValue(ROTATION)
    vi.mocked(api.addProviderKey).mockResolvedValue(ROTATION)
    vi.mocked(api.removeProviderKey).mockResolvedValue(ROTATION)
    vi.mocked(api.activateProviderKey).mockResolvedValue(ROTATION)
    vi.mocked(api.listProviders).mockResolvedValue({ active_provider_id: 'prov-anthropic', providers: [CONN] })
  })

  it('renders the rotation with the active marker and masked hints', async () => {
    renderPanel()
    await waitFor(() => expect(screen.getAllByTestId('provider-key-row')).toHaveLength(2))
    expect(screen.getByText('sk-ant…aaaa')).toBeInTheDocument()
    expect(screen.getByText('Active')).toBeInTheDocument()
    // Active row has no activate button; the spare does.
    expect(screen.queryByTestId('provider-key-activate-0')).toBeNull()
    expect(screen.getByTestId('provider-key-activate-1')).toBeInTheDocument()
    // The rotation note is always visible.
    expect(screen.getByText(/automatically tries the next key/i)).toBeInTheDocument()
  })

  it('add: a pasted key calls the command and clears the field', async () => {
    const withNew = [...ROTATION, { index: 2, active: false, masked_hint: 'sk-ant…cccc' }]
    vi.mocked(api.addProviderKey).mockResolvedValue(withNew)
    const onKeysChanged = vi.fn().mockResolvedValue(undefined)
    renderPanel(onKeysChanged)
    const input = await screen.findByTestId('provider-key-input')
    fireEvent.change(input, { target: { value: 'sk-ant-api03-new-full-key' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add key' }))
    await waitFor(() => expect(api.addProviderKey).toHaveBeenCalledWith('prov-anthropic', 'sk-ant-api03-new-full-key'))
    await waitFor(() => expect(screen.getAllByTestId('provider-key-row')).toHaveLength(3))
    await waitFor(() => expect(onKeysChanged).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(input).toHaveValue(''))
  })

  it('add: blank keys never reach the api (submit disabled, alert path for whitespace-only)', async () => {
    renderPanel()
    const input = await screen.findByTestId('provider-key-input')
    // Fully blank → the submit button is disabled outright (no round trip).
    expect(screen.getByRole('button', { name: 'Add key' })).toBeDisabled()
    fireEvent.change(input, { target: { value: '\t ' } })
    expect(screen.getByRole('button', { name: 'Add key' })).toBeDisabled()
    expect(api.addProviderKey).not.toHaveBeenCalled()
  })

  it('activate: switches the active slot and refreshes the roster', async () => {
    const swapped = [
      { index: 0, active: false, masked_hint: 'sk-ant…aaaa' },
      { index: 1, active: true, masked_hint: 'sk-ant…bbbb' },
    ]
    vi.mocked(api.activateProviderKey).mockResolvedValue(swapped)
    const onKeysChanged = vi.fn().mockResolvedValue(undefined)
    renderPanel(onKeysChanged)
    await waitFor(() => expect(screen.getByTestId('provider-key-activate-1')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('provider-key-activate-1'))
    await waitFor(() => expect(api.activateProviderKey).toHaveBeenCalledWith('prov-anthropic', 1))
    // Rows follow the command's fresh list — the spare now carries the star.
    await waitFor(() => expect(screen.getByTestId('provider-key-activate-0')).toBeInTheDocument())
    await waitFor(() => expect(onKeysChanged).toHaveBeenCalledTimes(1))
  })

  it('remove: the LAST remaining key has its affordance disabled (engine refuses)', async () => {
    vi.mocked(api.listProviderKeys).mockResolvedValue([
      { index: 0, active: true, masked_hint: 'sk-glm…3333' },
    ])
    renderPanel()
    await waitFor(() => expect(screen.getAllByTestId('provider-key-row')).toHaveLength(1))
    expect(screen.getByTestId('provider-key-remove-0')).toBeDisabled()
    // The hint says why.
    expect(screen.getByText(/last remaining key cannot be removed/i)).toBeInTheDocument()
  })

  it('remove: cancel is a no-op; the ACTIVE-key warning names the promotion', async () => {
    renderPanel()
    await waitFor(() => expect(screen.getByTestId('provider-key-remove-0')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('provider-key-remove-0'))
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog.textContent).toContain('sk-ant…aaaa')
    expect(dialog.textContent).toContain('sk-ant…bbbb', 'the promoted next key is named')
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(api.removeProviderKey).not.toHaveBeenCalled()
  })

  it('remove: confirming calls the command with the index', async () => {
    vi.mocked(api.removeProviderKey).mockResolvedValue([ROTATION[0]])
    renderPanel()
    await waitFor(() => expect(screen.getByTestId('provider-key-remove-1')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('provider-key-remove-1'))
    await waitFor(() => expect(screen.getByRole('alertdialog')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Remove key' }))
    await waitFor(() => expect(api.removeProviderKey).toHaveBeenCalledWith('prov-anthropic', 1))
    await waitFor(() => expect(screen.getAllByTestId('provider-key-row')).toHaveLength(1))
  })

  it('masked hints rendered by the panel never contain full key material', async () => {
    renderPanel()
    await waitFor(() => expect(screen.getAllByTestId('provider-key-row')).toHaveLength(2))
    // The backend owns masking — the wire only ever carried hints. Assert
    // the rendered hints are the masked forms, not any plausible full key.
    for (const hint of ['sk-ant…aaaa', 'sk-ant…bbbb']) {
      expect(screen.getByText(hint)).toBeInTheDocument()
    }
    expect(document.body.textContent).not.toContain('sk-ant-api03')
  })
})
