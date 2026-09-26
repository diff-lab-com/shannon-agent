// B2 P2-10: the execution-mode switcher's menu was a keyboard dead end —
// opening it left focus on the trigger, so ArrowUp/Down/Enter/Escape never
// reached the listbox. It now follows the listbox roving-focus pattern:
// open moves focus to the selected item, arrows cycle, Enter/Space pick,
// Escape closes and restores focus to the trigger.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { ExecutionModeSwitcher } from '@/components/chat/ExecutionModeSwitcher'
import * as api from '@/lib/tauri-api'
import type * as ReactRouterDom from 'react-router-dom'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return { ...actual, useNavigate: () => vi.fn() }
})

const mockRefreshConfig = vi.fn()
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { active_permission_profile: 'strict', approval_mode: 'auto' },
    refreshConfig: mockRefreshConfig,
  }),
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    activatePermissionProfile: vi.fn().mockResolvedValue(undefined),
  }
})

function renderSwitcher() {
  return render(
    <I18nProvider>
      <ExecutionModeSwitcher />
    </I18nProvider>,
  )
}

const options = () => screen.getAllByRole('option')

beforeEach(() => {
  vi.mocked(api.activatePermissionProfile).mockClear()
  vi.mocked(api.activatePermissionProfile).mockResolvedValue(undefined)
  mockRefreshConfig.mockClear()
})

describe('ExecutionModeSwitcher keyboard support', () => {
  it('moves focus to the selected item when the menu opens', () => {
    const { container } = renderSwitcher()
    fireEvent.click(container.querySelector('button')!)
    const listbox = screen.getByRole('listbox', { name: 'Execution mode options' })
    expect(listbox).toBeInTheDocument()
    // active_permission_profile=strict → the first option is selected and focused.
    const focused = document.activeElement
    expect(focused).toBe(options()[0])
    expect(options()[0]).toHaveAttribute('aria-selected', 'true')
  })

  it('cycles focus with ArrowDown/ArrowUp (wrapping)', () => {
    const { container } = renderSwitcher()
    fireEvent.click(container.querySelector('button')!)
    expect(document.activeElement).toBe(options()[0])
    fireEvent.keyDown(screen.getByRole('listbox', { name: 'Execution mode options' }), { key: 'ArrowDown' })
    expect(document.activeElement).toBe(options()[1])
    fireEvent.keyDown(screen.getByRole('listbox', { name: 'Execution mode options' }), { key: 'ArrowUp' })
    expect(document.activeElement).toBe(options()[0])
    // Wrap upwards past the first option → last option.
    fireEvent.keyDown(screen.getByRole('listbox', { name: 'Execution mode options' }), { key: 'ArrowUp' })
    expect(document.activeElement).toBe(options()[options().length - 1])
  })

  it('picks the focused option with Enter, closes, and restores focus', async () => {
    const { container } = renderSwitcher()
    fireEvent.click(container.querySelector('button')!)
    fireEvent.keyDown(screen.getByRole('listbox', { name: 'Execution mode options' }), { key: 'ArrowDown' })
    fireEvent.keyDown(screen.getByRole('listbox', { name: 'Execution mode options' }), { key: 'Enter' })
    await waitFor(() => {
      expect(api.activatePermissionProfile).toHaveBeenCalledWith('balanced')
    })
    await waitFor(() => {
      expect(screen.queryByRole('listbox', { name: 'Execution mode options' })).toBeNull()
    })
    // Focus lands back on the trigger button.
    expect(container.querySelector('button')).toBe(document.activeElement)
  })

  it('closes on Escape and restores focus to the trigger without picking', () => {
    const { container } = renderSwitcher()
    fireEvent.click(container.querySelector('button')!)
    fireEvent.keyDown(screen.getByRole('listbox', { name: 'Execution mode options' }), { key: 'Escape' })
    expect(screen.queryByRole('listbox', { name: 'Execution mode options' })).toBeNull()
    expect(api.activatePermissionProfile).not.toHaveBeenCalled()
    expect(container.querySelector('button')).toBe(document.activeElement)
  })

  it('still closes on outside click', () => {
    renderSwitcher()
    fireEvent.click(screen.getByRole('button', { name: /Execution mode:/ }))
    expect(screen.getByRole('listbox', { name: 'Execution mode options' })).toBeInTheDocument()
    fireEvent.mouseDown(document.body)
    expect(screen.queryByRole('listbox', { name: 'Execution mode options' })).toBeNull()
  })
})
