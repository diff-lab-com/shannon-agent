// B2 / P1-11 — ParameterSlider writes once per gesture, not once per notch:
//   - onChange only moves the thumb;
//   - the persisted write fires on pointer/keyboard release and is deduped;
//   - a failed write toasts and snaps the slider back to the persisted value.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { ParameterSlider } from '@/components/settings/models-settings/ParameterSlider'
import * as api from '@/lib/tauri-api'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

function renderSlider(over: Partial<Parameters<typeof ParameterSlider>[0]> = {}) {
  return render(
    <ParameterSlider
      label="Temperature"
      value={0.7}
      min={0}
      max={1}
      step={0.1}
      configKey="temperature"
      {...over}
    />,
  )
}

function slider(): HTMLInputElement {
  return screen.getByRole('slider') as HTMLInputElement
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(api.configure).mockResolvedValue(undefined)
})

describe('ParameterSlider', () => {
  it('does not write during change — only on release', () => {
    renderSlider()
    fireEvent.change(slider(), { target: { value: '0.3' } })
    fireEvent.change(slider(), { target: { value: '0.1' } })
    expect(api.configure).not.toHaveBeenCalled()
    // The thumb shows the draft value while dragging.
    expect(slider().value).toBe('0.1')
    fireEvent.pointerUp(slider())
    expect(api.configure).toHaveBeenCalledTimes(1)
    expect(api.configure).toHaveBeenCalledWith({ key: 'temperature', value: '0.1' })
  })

  it('dedupes a release + blur burst to a single write', () => {
    renderSlider()
    fireEvent.change(slider(), { target: { value: '0.2' } })
    fireEvent.pointerUp(slider())
    fireEvent.blur(slider())
    expect(api.configure).toHaveBeenCalledTimes(1)
  })

  it('does not write at all when the value matches the persisted one', () => {
    renderSlider({ value: 0.3 })
    fireEvent.change(slider(), { target: { value: '0.3' } })
    fireEvent.pointerUp(slider())
    expect(api.configure).not.toHaveBeenCalled()
  })

  it('never writes when configKey is absent', () => {
    renderSlider({ configKey: undefined })
    fireEvent.change(slider(), { target: { value: '0.4' } })
    fireEvent.pointerUp(slider())
    expect(api.configure).not.toHaveBeenCalled()
  })

  it('toasts and reverts to the persisted value when the write fails', async () => {
    const { toast } = await import('sonner')
    vi.mocked(api.configure).mockRejectedValue('disk full')
    renderSlider()
    fireEvent.change(slider(), { target: { value: '0.1' } })
    fireEvent.pointerUp(slider())
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    await waitFor(() => expect(slider().value).toBe('0.7'))
  })
})
