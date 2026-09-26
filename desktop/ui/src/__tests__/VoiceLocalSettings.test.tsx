// B2 / P1-16 — VoiceLocalSettings language field: the draft saves once on
// blur / Enter, not once per keystroke with a toast per write.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { VoiceLocalSettings } from '@/components/settings/VoiceLocalSettings'
import * as api from '@/lib/tauri-api'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

const CONFIG: api.VoiceLocalConfig = {
  enabled: true,
  model: null,
  language: 'en',
  auto_download: true,
} as api.VoiceLocalConfig

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(api.getVoiceLocalConfig).mockResolvedValue(CONFIG)
  vi.mocked(api.listWhisperModels).mockResolvedValue([])
  vi.mocked(api.saveVoiceLocalConfig).mockResolvedValue(undefined)
})

function languageInput(): HTMLInputElement {
  return screen.getByLabelText('Language hint (BCP-47)') as HTMLInputElement
}

async function renderLoaded() {
  render(<VoiceLocalSettings />)
  await waitFor(() => expect(languageInput().value).toBe('en'))
}

describe('VoiceLocalSettings — language field', () => {
  it('does not save while typing — only on blur', async () => {
    await renderLoaded()
    fireEvent.change(languageInput(), { target: { value: 'zh' } })
    expect(api.saveVoiceLocalConfig).not.toHaveBeenCalled()
    fireEvent.blur(languageInput())
    await waitFor(() =>
      expect(api.saveVoiceLocalConfig).toHaveBeenCalledWith(
        expect.objectContaining({ language: 'zh' }),
      ),
    )
  })

  it('commits on Enter and skips the write when nothing changed', async () => {
    await renderLoaded()
    fireEvent.change(languageInput(), { target: { value: 'zh' } })
    fireEvent.keyDown(languageInput(), { key: 'Enter' })
    await waitFor(() =>
      expect(api.saveVoiceLocalConfig).toHaveBeenCalledWith(
        expect.objectContaining({ language: 'zh' }),
      ),
    )
    vi.mocked(api.saveVoiceLocalConfig).mockClear()
    // Blur afterwards with the same value: no duplicate write, no toast.
    fireEvent.blur(languageInput())
    expect(api.saveVoiceLocalConfig).not.toHaveBeenCalled()
  })

  it('toasts once (not per keystroke) when the commit fails', async () => {
    const { toast } = await import('sonner')
    vi.mocked(api.saveVoiceLocalConfig).mockRejectedValue('disk error')
    await renderLoaded()
    fireEvent.change(languageInput(), { target: { value: 'zh' } })
    fireEvent.blur(languageInput())
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    // One failed write, one failure toast — the persisted config is untouched
    // and the draft keeps the user's edit for a retry.
    expect(api.saveVoiceLocalConfig).toHaveBeenCalledTimes(1)
    expect(languageInput().value).toBe('zh')
  })
})
