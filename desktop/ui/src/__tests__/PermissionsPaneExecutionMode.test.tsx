// design-parity R2 (Wave B′, audit §3.2): the 默认执行模式 entity card
// moved from the General page into 权限与安全 (PermissionsPane) — the two
// guidance cards pointed at each other in a ring, and the design places the
// tier inside the security pane (12-settings-security.html:163-173).
//
// This suite re-homes the Round-2 read-resolver coverage that used to live
// in GeneralSettings.test.tsx (the control changed pages, the contract did
// not): in-table config values select their segment; out-of-table engine
// values (confirm / dont_ask / …) select NOTHING and name the raw value;
// `plan` (Aurora 裁决 B1: a legal engine approval_mode) is the fourth stop;
// every pick writes the SAME `approval_mode` key the composer pill writes.

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import PermissionsPane from '@/pages/settings/PermissionsPane'

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter>
          {/* PermissionsSettings (rendered below the entity card) reads no
              artifact state, but the pane's neighbors do — keep the wrap
              identical to the GeneralSettings suite so the mock defaults
              line up. */}
          <ArtifactProvider>
            {ui}
          </ArtifactProvider>
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

const configWith = (approvalMode: string) => ({
  provider: 'anthropic',
  model: 'claude-sonnet-4-6',
  api_key: 'sk-test',
  working_dir: '/tmp',
  approval_mode: approvalMode,
})

describe('PermissionsPane 默认执行模式 entity card (design-parity R2)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getConfig).mockResolvedValue(configWith('auto-edit'))
  })

  it('an in-table config selects its tier (auto_edit → Auto Edit)', async () => {
    vi.mocked(api.getConfig).mockResolvedValue(configWith('auto_edit'))
    render(wrap(<PermissionsPane />))
    await waitFor(() => expect(screen.getByRole('radio', { name: 'Auto Edit' })).toHaveAttribute('aria-checked', 'true'))
    expect(screen.getByRole('radio', { name: 'Ask' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: 'Full' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: 'Plan' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.queryByTestId('approval-mode-raw-hint')).not.toBeInTheDocument()
  })

  it('legacy factory-default "confirm" is a legacy alias: it selects the Ask tier', async () => {
    vi.mocked(api.getConfig).mockResolvedValue(configWith('confirm'))
    render(wrap(<PermissionsPane />))
    // normalizeApprovalMode maps confirm ≡ ask (R3), so the Ask segment
    // selects — never a raw-value readout for a known alias.
    await waitFor(() => expect(screen.getByRole('radio', { name: 'Ask' })).toHaveAttribute('aria-checked', 'true'))
    expect(screen.queryByTestId('approval-mode-raw-hint')).not.toBeInTheDocument()
  })

  it('an expert config (bypass_permissions) selects no quick stop and shows its description', async () => {
    vi.mocked(api.getConfig).mockResolvedValue(configWith('bypass_permissions'))
    render(wrap(<PermissionsPane />))
    // bypassPermissions is an expert mode, not one of the four stops: no
    // segment selects, but the read is still honest — the bypass tier's
    // own description names what the engine is actually doing.
    await waitFor(() =>
      expect(screen.getByText('Skip all checks except deny rules — trusted environments only')).toBeInTheDocument(),
    )
    const group = screen.getByRole('radiogroup', { name: 'Approval mode' })
    for (const radio of within(group).getAllByRole('radio')) {
      expect(radio).toHaveAttribute('aria-checked', 'false')
    }
    expect(screen.queryByTestId('approval-mode-raw-hint')).not.toBeInTheDocument()
  })

  it('plan is the fourth stop (Aurora 裁决 B1) and selects when configured', async () => {
    vi.mocked(api.getConfig).mockResolvedValue(configWith('plan'))
    render(wrap(<PermissionsPane />))
    await waitFor(() => expect(screen.getByRole('radio', { name: 'Plan' })).toHaveAttribute('aria-checked', 'true'))
    expect(screen.queryByTestId('approval-mode-raw-hint')).not.toBeInTheDocument()
  })

  it('picking a segment writes the composer-shared approval_mode key', async () => {
    vi.mocked(api.getConfig).mockResolvedValue(configWith('auto-edit'))
    render(wrap(<PermissionsPane />))
    const ask = await screen.findByRole('radio', { name: 'Ask' })
    fireEvent.click(ask)
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'ask' })
    })
  })

  it('the two-configs distinction note (README §5-5) and the expert picker are present', async () => {
    render(wrap(<PermissionsPane />))
    const card = await screen.findByTestId('permissions-execution-mode-card')
    expect(within(card).getByTestId('permissions-execution-mode-distinction')).toHaveTextContent(
      /approval ladder \(approval_mode\)/,
    )
    expect(within(card).getByLabelText('Advanced')).toBeInTheDocument()
  })
})
