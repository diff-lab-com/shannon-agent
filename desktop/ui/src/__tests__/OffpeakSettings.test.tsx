// P2-5: Settings → Advanced off-peak model override input.
//
// Uses the global setup.ts tauri-api mock (AppProvider pulls a wide API
// surface); asserts that saving persists the frozen `offpeak.model_override`
// config key with a trimmed value (empty = disabled).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import AdvancedSettings from '@/components/settings/AdvancedSettings'
import * as api from '@/lib/tauri-api'

function wrap(ui: React.ReactElement) {
  return (
    <AppProvider>
      <MemoryRouter>
        {ui}
      </MemoryRouter>
    </AppProvider>
  )
}

beforeEach(() => {
  vi.mocked(api.configure).mockClear()
  vi.mocked(api.configure).mockResolvedValue(undefined)
})

describe('P2-5 AdvancedSettings offpeak.model_override', () => {
  it('renders the off-peak override card', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Off-peak Model Override')).toBeInTheDocument()
    expect(screen.getByLabelText('Model id for in-window routine runs')).toBeInTheDocument()
  })

  it('saves a trimmed value under the frozen config key', async () => {
    render(wrap(<AdvancedSettings />))

    const input = screen.getByLabelText('Model id for in-window routine runs')
    fireEvent.change(input, { target: { value: '  glm-4-flash ' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save off-peak model override' }))

    await waitFor(() =>
      expect(api.configure).toHaveBeenCalledWith({
        key: 'offpeak.model_override',
        value: 'glm-4-flash',
      }),
    )
  })

  it('sends an empty string when the user clears the input (disabled)', async () => {
    render(wrap(<AdvancedSettings />))

    const input = screen.getByLabelText('Model id for in-window routine runs')
    fireEvent.change(input, { target: { value: '   ' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save off-peak model override' }))

    await waitFor(() =>
      expect(api.configure).toHaveBeenCalledWith({
        key: 'offpeak.model_override',
        value: '',
      }),
    )
  })
})
