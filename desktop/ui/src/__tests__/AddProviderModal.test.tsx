import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { ThemeProvider } from '@/context/ThemeContext'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import AddProviderModal from '@/components/settings/AddProviderModal'
import * as api from '@/lib/tauri-api'
import type { ProvidersFile } from '@/types'

function wrap(ui: React.ReactElement) {
  return (
    <ThemeProvider>
      <AppProvider>
        <MemoryRouter>
          {ui}
        </MemoryRouter>
      </AppProvider>
    </ThemeProvider>
  )
}

const EMPTY_FILE: ProvidersFile = { active_provider_id: null, providers: [] }

/**
 * The modal defaults to `openai-compatible` which requires a base URL
 * before submit will fire. Tests that exercise submit() need both
 * the label and a base URL filled in. This helper does both.
 */
function fillRequiredFields(label = 'Acme') {
  fireEvent.change(screen.getByPlaceholderText('My GLM key'), {
    target: { value: label },
  })
  fireEvent.change(screen.getByPlaceholderText('https://api.example.com/v1'), {
    target: { value: 'https://api.example.com/v1' },
  })
}

function renderModal() {
  const onClose = vi.fn()
  const onSaved = vi.fn()
  const utils = render(
    wrap(
      <AddProviderModal
        editing={null}
        onClose={onClose}
        onSaved={onSaved}
      />,
    ),
  )
  return { ...utils, onClose, onSaved }
}

beforeEach(() => {
  vi.mocked(api.saveProvider).mockReset()
  vi.mocked(api.saveProvider).mockResolvedValue(EMPTY_FILE)
})

describe('AddProviderModal — Advanced disclosure (Phase 2 task 3)', () => {
  it('renders the Advanced toggle when the modal is open', () => {
    renderModal()
    expect(screen.getByTestId('add-provider-advanced-toggle')).toBeInTheDocument()
    expect(screen.getByText('Advanced')).toBeInTheDocument()
  })

  it('hides the 3 subsections until the Advanced toggle is clicked', () => {
    renderModal()
    // Default max tokens input + tier rows are not visible yet.
    expect(screen.queryByTestId('default-max-tokens')).not.toBeInTheDocument()
    expect(screen.queryByTestId('extra-headers-empty')).not.toBeInTheDocument()
    expect(screen.queryByTestId('tier-fast-row')).not.toBeInTheDocument()
    // Toggle the disclosure open.
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    expect(screen.getByTestId('default-max-tokens')).toBeInTheDocument()
    expect(screen.getByTestId('extra-headers-empty')).toBeInTheDocument()
    expect(screen.getByTestId('tier-fast-row')).toBeInTheDocument()
    expect(screen.getByTestId('tier-standard-row')).toBeInTheDocument()
    expect(screen.getByTestId('tier-pro-row')).toBeInTheDocument()
  })

  it('adds an extra header row and includes it in the saved payload', async () => {
    const { onSaved } = renderModal()
    // Fill label + baseUrl so submit() actually fires.
    fillRequiredFields()
    // Open advanced, add a row, fill in key/value.
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    fireEvent.click(screen.getByTestId('extra-headers-add'))
    const row = screen.getByTestId('extra-headers-row')
    const inputs = row.querySelectorAll('input')
    fireEvent.change(inputs[0], { target: { value: 'X-Region' } })
    fireEvent.change(inputs[1], { target: { value: 'us-east' } })
    // Submit.
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(onSaved).toHaveBeenCalled())
    const [input] = vi.mocked(api.saveProvider).mock.calls[0]
    expect(input.extra_headers).toEqual({ 'X-Region': 'us-east' })
  })

  it('drops a removed extra header row from the payload', async () => {
    const { onSaved } = renderModal()
    fillRequiredFields()
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    fireEvent.click(screen.getByTestId('extra-headers-add'))
    const row = screen.getByTestId('extra-headers-row')
    const inputs = row.querySelectorAll('input')
    fireEvent.change(inputs[0], { target: { value: 'X-Region' } })
    fireEvent.change(inputs[1], { target: { value: 'us-east' } })
    // Remove the only row via the close (×) button on the row.
    fireEvent.click(row.querySelector('button')!)
    // Empty state returns.
    expect(screen.getByTestId('extra-headers-empty')).toBeInTheDocument()
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(onSaved).toHaveBeenCalled())
    const [input] = vi.mocked(api.saveProvider).mock.calls[0]
    expect(input.extra_headers).toEqual({})
  })

  it('drops an extra header row whose key is empty', async () => {
    const { onSaved } = renderModal()
    fillRequiredFields()
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    fireEvent.click(screen.getByTestId('extra-headers-add'))
    fireEvent.click(screen.getByTestId('extra-headers-add'))
    const rows = screen.getAllByTestId('extra-headers-row')
    // First row: only value, no key — must be dropped.
    const r0 = rows[0].querySelectorAll('input')
    fireEvent.change(r0[1], { target: { value: 'orphan' } })
    // Second row: real key+value — must survive.
    const r1 = rows[1].querySelectorAll('input')
    fireEvent.change(r1[0], { target: { value: 'X-Real' } })
    fireEvent.change(r1[1], { target: { value: 'yes' } })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(onSaved).toHaveBeenCalled())
    const [input] = vi.mocked(api.saveProvider).mock.calls[0]
    expect(input.extra_headers).toEqual({ 'X-Real': 'yes' })
  })

  it('writes default_max_tokens: 4096 to the payload when set', async () => {
    const { onSaved } = renderModal()
    fillRequiredFields()
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    const dmt = screen.getByTestId('default-max-tokens')
    fireEvent.change(dmt, { target: { value: '4096' } })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(onSaved).toHaveBeenCalled())
    const [input] = vi.mocked(api.saveProvider).mock.calls[0]
    expect(input.default_max_tokens).toBe(4096)
  })

  it('writes default_max_tokens: null when the input is blank', async () => {
    const { onSaved } = renderModal()
    fillRequiredFields()
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    // Touch the field then clear — should round-trip to null.
    const dmt = screen.getByTestId('default-max-tokens')
    fireEvent.change(dmt, { target: { value: '' } })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(onSaved).toHaveBeenCalled())
    const [input] = vi.mocked(api.saveProvider).mock.calls[0]
    expect(input.default_max_tokens).toBeNull()
  })

  it('writes the standard tier model id to the payload', async () => {
    const { onSaved } = renderModal()
    fillRequiredFields()
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    fireEvent.change(screen.getByTestId('tier-standard-input'), {
      target: { value: 'claude-sonnet-4-6' },
    })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(onSaved).toHaveBeenCalled())
    const [input] = vi.mocked(api.saveProvider).mock.calls[0]
    expect(input.tiers).toEqual({
      fast: null,
      standard: 'claude-sonnet-4-6',
      pro: null,
    })
  })

  it('does not surface Tier aliases in the UI (fast/standard/pro only)', async () => {
    const { onSaved } = renderModal()
    fillRequiredFields()
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    // The three rows render with the canonical labels — no "Haiku" /
    // "Sonnet" / "Opus" / "Flash" aliases here.
    expect(screen.getByText('Fast')).toBeInTheDocument()
    expect(screen.getByText('Standard')).toBeInTheDocument()
    expect(screen.getByText('Pro')).toBeInTheDocument()
    // Belt-and-braces: the saved payload uses canonical keys.
    fireEvent.change(screen.getByTestId('tier-fast-input'), {
      target: { value: 'haiku-fast' },
    })
    fireEvent.change(screen.getByTestId('tier-standard-input'), {
      target: { value: 'sonnet-fast' },
    })
    fireEvent.change(screen.getByTestId('tier-pro-input'), {
      target: { value: 'opus-fast' },
    })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(onSaved).toHaveBeenCalled())
    const [input] = vi.mocked(api.saveProvider).mock.calls[0]
    expect(input.tiers).toEqual({
      fast: 'haiku-fast',
      standard: 'sonnet-fast',
      pro: 'opus-fast',
    })
  })

  it('shows an active-tier badge when the model field matches a tier override', async () => {
    // P4.11: badge the tier row whose override matches the provider's
    // active model so the user knows which tier their engine is using.
    renderModal()
    fillRequiredFields()
    // Set the model field to match a tier value. The model input has no
    // data-testid; identify it by its placeholder (`claude-sonnet-4-6`) — same
    // trick used by the production quick-fill chip.
    fireEvent.change(screen.getByPlaceholderText('claude-sonnet-4-6'), {
      target: { value: 'claude-sonnet-4-6' },
    })
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    fireEvent.change(screen.getByTestId('tier-standard-input'), {
      target: { value: 'claude-sonnet-4-6' },
    })
    expect(screen.getByTestId('tier-standard-active')).toBeInTheDocument()
    expect(screen.queryByTestId('tier-fast-active')).not.toBeInTheDocument()
    expect(screen.queryByTestId('tier-pro-active')).not.toBeInTheDocument()
  })

  it('clears a tier override when the row clear button is clicked', async () => {
    // P4.11: per-row clear button resets a single tier to the catalog
    // default without touching the others.
    renderModal()
    fillRequiredFields()
    fireEvent.click(screen.getByTestId('add-provider-advanced-toggle'))
    fireEvent.change(screen.getByTestId('tier-fast-input'), {
      target: { value: 'haiku-4-5' },
    })
    fireEvent.change(screen.getByTestId('tier-pro-input'), {
      target: { value: 'opus-4-8' },
    })
    expect(screen.getByTestId('tier-fast-clear')).toBeInTheDocument()
    expect(screen.getByTestId('tier-pro-clear')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('tier-fast-clear'))
    expect((screen.getByTestId('tier-fast-input') as HTMLInputElement).value).toBe('')
    // Pro row is untouched.
    expect((screen.getByTestId('tier-pro-input') as HTMLInputElement).value).toBe('opus-4-8')
    // Clear button hides on empty rows.
    expect(screen.queryByTestId('tier-fast-clear')).not.toBeInTheDocument()
  })
})

// === 2026-09-29 provider review §2-9 / §2-12 — fetch model list + in-modal
// connection test. Both are wired through `canProbe` (base URL present, and
// a key for key-needing kinds), and both stay purely in-memory. ===
describe('AddProviderModal — fetch models + test connection', () => {
  beforeEach(() => {
    vi.mocked(api.fetchProviderModels).mockReset()
    vi.mocked(api.fetchProviderModels).mockResolvedValue([])
    vi.mocked(api.testProviderCredentials).mockReset()
    vi.mocked(api.testProviderCredentials).mockResolvedValue({ kind: 'success' })
  })

  it('disables both probe buttons until base URL AND key are present', () => {
    renderModal()
    // openai-compatible needs key AND base URL.
    expect(screen.getByTestId('fetch-models')).toBeDisabled()
    expect(screen.getByTestId('test-provider-connection')).toBeDisabled()
    // Base URL alone is not enough for a key-needing kind.
    fillRequiredFields()
    expect(screen.getByTestId('fetch-models')).toBeDisabled()
    expect(screen.getByTestId('test-provider-connection')).toBeDisabled()
    fireEvent.change(screen.getByPlaceholderText('sk-…'), { target: { value: 'sk-test' } })
    expect(screen.getByTestId('fetch-models')).toBeEnabled()
    expect(screen.getByTestId('test-provider-connection')).toBeEnabled()
  })

  it('disables the probe buttons for key-needing kinds without a key', () => {
    renderModal()
    // Base URL filled, but the default openai-compatible kind needs a key.
    fireEvent.change(screen.getByPlaceholderText('https://api.example.com/v1'), {
      target: { value: 'https://api.example.com/v1' },
    })
    expect(screen.getByTestId('fetch-models')).toBeDisabled()
    // Enter a key → enabled.
    fireEvent.change(screen.getByPlaceholderText('sk-…'), { target: { value: 'sk-test' } })
    expect(screen.getByTestId('fetch-models')).toBeEnabled()
    expect(screen.getByTestId('test-provider-connection')).toBeEnabled()
  })

  it('fetches models and offers them as suggestions while keeping free text', async () => {
    vi.mocked(api.fetchProviderModels).mockResolvedValue(['model-b', 'model-a'])
    renderModal()
    fillRequiredFields()
    fireEvent.change(screen.getByPlaceholderText('sk-…'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByTestId('fetch-models'))
    await waitFor(() => expect(screen.getByTestId('models-found')).toBeInTheDocument())
    expect(screen.getByTestId('models-found').textContent).toContain('2')
    // Suggestions ride the input's datalist.
    const input = screen.getByTestId('provider-model-input') as HTMLInputElement
    expect(input.getAttribute('list')).toBeTruthy()
    expect(document.querySelector(`datalist#${CSS.escape(input.getAttribute('list')!)} option[value="model-a"]`)).not.toBeNull()
    // Wire shape: null provider id (add mode), kind, trimmed base URL, key.
    expect(vi.mocked(api.fetchProviderModels)).toHaveBeenCalledWith(
      null, 'openai-compatible', 'https://api.example.com/v1', 'sk-test',
    )
    // Free text still works.
    fireEvent.change(input, { target: { value: 'my-custom-model' } })
    expect(input.value).toBe('my-custom-model')
  })

  it('shows a categorized inline error when the fetch fails', async () => {
    vi.mocked(api.fetchProviderModels).mockRejectedValue('invalid_key')
    renderModal()
    fillRequiredFields()
    fireEvent.change(screen.getByPlaceholderText('sk-…'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByTestId('fetch-models'))
    await waitFor(() => expect(screen.getByTestId('fetch-models-error')).toBeInTheDocument())
    expect(screen.getByTestId('fetch-models-error').textContent).toContain('Invalid API key')
    expect(screen.queryByTestId('models-found')).not.toBeInTheDocument()
  })

  it('passes the editing id for the stored-key fallback in edit mode', async () => {
    vi.mocked(api.fetchProviderModels).mockResolvedValue(['glm-4.7'])
    render(
      wrap(
        <AddProviderModal
          editing={{
            id: 'glm-key', display_name: 'GLM', kind: 'openai-compatible',
            has_api_key: true, base_url: 'https://open.bigmodel.cn/api/paas/v4',
          }}
          onClose={vi.fn()}
          onSaved={vi.fn()}
        />,
      ),
    )
    fireEvent.click(screen.getByTestId('fetch-models'))
    await waitFor(() => expect(screen.getByTestId('models-found')).toBeInTheDocument())
    expect(vi.mocked(api.fetchProviderModels)).toHaveBeenCalledWith(
      'glm-key', 'openai-compatible', 'https://open.bigmodel.cn/api/paas/v4', null,
    )
  })

  it('tests credentials in place without saving', async () => {
    vi.mocked(api.testProviderCredentials).mockResolvedValue({ kind: 'success' })
    renderModal()
    fillRequiredFields()
    fireEvent.change(screen.getByPlaceholderText('sk-…'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByTestId('test-provider-connection'))
    const status = await waitFor(() => screen.getByTestId('provider-test-status'))
    expect(status.textContent).toMatch(/Connected \(\d+ ms\)/)
    expect(vi.mocked(api.testProviderCredentials)).toHaveBeenCalledWith(
      'openai-compatible', 'https://api.example.com/v1', 'sk-test', null,
    )
    // Testing must NOT save.
    expect(vi.mocked(api.saveProvider)).not.toHaveBeenCalled()
  })

  it('renders the categorized verdict for an invalid key', async () => {
    vi.mocked(api.testProviderCredentials).mockResolvedValue({ kind: 'invalid_key' })
    renderModal()
    fillRequiredFields()
    fireEvent.change(screen.getByPlaceholderText('sk-…'), { target: { value: 'sk-bad' } })
    fireEvent.click(screen.getByTestId('test-provider-connection'))
    const status = await waitFor(() => screen.getByTestId('provider-test-status'))
    expect(status.textContent).toContain('Invalid API key')
  })

  it('renders the categorized verdict for an exhausted quota (HTTP 402, R2-P1-10)', async () => {
    vi.mocked(api.testProviderCredentials).mockResolvedValue({ kind: 'quota_exhausted' })
    renderModal()
    fillRequiredFields()
    fireEvent.change(screen.getByPlaceholderText('sk-…'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByTestId('test-provider-connection'))
    const status = await waitFor(() => screen.getByTestId('provider-test-status'))
    expect(status.textContent).toContain('Quota exhausted')
  })

  it('clears stale fetch results when the base URL changes', async () => {
    vi.mocked(api.fetchProviderModels).mockResolvedValue(['model-a'])
    renderModal()
    fillRequiredFields()
    fireEvent.change(screen.getByPlaceholderText('sk-…'), { target: { value: 'sk-test' } })
    fireEvent.click(screen.getByTestId('fetch-models'))
    await waitFor(() => expect(screen.getByTestId('models-found')).toBeInTheDocument())
    fireEvent.change(screen.getByPlaceholderText('https://api.example.com/v1'), {
      target: { value: 'https://other.example.com/v1' },
    })
    expect(screen.queryByTestId('models-found')).not.toBeInTheDocument()
    const input = screen.getByTestId('provider-model-input') as HTMLInputElement
    expect(input.getAttribute('list')).toBeNull()
  })
})