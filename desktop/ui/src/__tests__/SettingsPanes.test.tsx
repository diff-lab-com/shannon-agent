import { describe, it, expect } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { AppProvider } from '@/context/AppContext'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import GeneralPane from '@/pages/settings/GeneralPane'
import ConnectionsPane from '@/pages/settings/ConnectionsPane'
import PermissionsPane from '@/pages/settings/PermissionsPane'

// IA redesign 2026-10 (ADVERSARIAL-REVIEW §2): the merged panes stack the
// original settings components under group headings. These minimal render
// tests pin the merge contract: each pane shows the group headings AND the
// distinctive markers of every absorbed component, so a regression that
// drops one side of the merge fails here. (The components' own suites keep
// covering their internals; en is the default test locale.)

function wrap(ui: React.ReactElement, initialEntries: string[] = ['/']) {
  return (
    <AppProvider>
      <MemoryRouter initialEntries={initialEntries}>
        {/* Batch D4: GeneralSettings consumes the app-level artifact
            context (auto-open toggle). */}
        <ArtifactProvider>{ui}</ArtifactProvider>
      </MemoryRouter>
    </AppProvider>
  )
}

describe('merged settings panes (IA 8-section merge)', () => {
  it('GeneralPane renders the General group above the session-defaults group', async () => {
    render(wrap(<GeneralPane />))
    // Group headings (nav.general / nav.session).
    expect(screen.getByRole('heading', { name: 'General' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Sessions' })).toBeInTheDocument()
    // GeneralSettings' approval segmented control (the section's marker card).
    expect(await screen.findByText('Approval Mode')).toBeInTheDocument()
    // SessionSettings' lead-in (settings.session.title).
    expect(screen.getByText('Session lifecycle and storage')).toBeInTheDocument()
  })

  it('GeneralPane closes with the honest 数据 card (design 12-settings-general:236-243)', async () => {
    render(wrap(<GeneralPane />))
    expect(await screen.findByText('Approval Mode')).toBeInTheDocument()
    // Group heading + card marker.
    expect(screen.getByRole('heading', { name: 'Data' })).toBeInTheDocument()
    // 缓期批 2: the engine ships export_all_sessions, so the IA redesign's
    // amber「即将支持」honest placeholder became a REAL button (the export
    // flow itself is pinned by GeneralPaneDataExport.test.tsx).
    expect(screen.getByTestId('export-all-sessions')).toHaveTextContent('Export all sessions')
    expect(screen.queryByTestId('data-export-full-badge')).not.toBeInTheDocument()
    // Clear-cache moved here from the dev-gated advanced page.
    expect(screen.getByTestId('data-clear-cache-button')).toHaveTextContent('Clear Chat Cache')
    // The on-device note.
    expect(screen.getByText(/Session data lives only on this machine/)).toBeInTheDocument()
  })

  it('GeneralPane carries the 启动 card with both launch toggles (批 1)', async () => {
    render(wrap(<GeneralPane />))
    // The section sits between the session group and 数据.
    expect(await screen.findByRole('heading', { name: 'Launch' })).toBeInTheDocument()
    expect(screen.getByTestId('general-launch-card')).toBeInTheDocument()
    // ① Session-window restore — default ON.
    const restore = screen.getByTestId('general-launch-restore-switch')
    expect(restore).toHaveAttribute('aria-checked', 'true')
    expect(restore).toHaveAccessibleName('Restore session windows at launch')
    // ② Check-only update check — default ON, and the copy keeps the
    // product promise: notify only, never auto-install.
    const check = screen.getByTestId('general-launch-update-check-switch')
    expect(check).toHaveAttribute('aria-checked', 'true')
    expect(check).toHaveAccessibleName('Check for updates at launch')
    expect(screen.getByText('Notifies only — the app never installs updates automatically')).toBeInTheDocument()
    // Both toggles write-once-at-launch semantics get the restart-app badge
    // (getAllByText: the hw-accel card in GeneralSettings carries one too).
    expect(screen.getAllByText('Restart required').length).toBeGreaterThanOrEqual(1)
  })

  it('ConnectionsPane renders gateway, remote-targets and network groups', async () => {
    render(wrap(<ConnectionsPane />))
    // Group heading for the gateway group (settings.connections.title).
    // Design-parity R1: converged with the rail's nav.connections — one term
    // (连接/Connections), no more pane saying Gateway under a Connections rail.
    expect(screen.getByRole('heading', { name: 'Connections' })).toBeInTheDocument()
    // RemotesSettings self-titles with its own h2.
    expect(screen.getByRole('heading', { name: 'Remote targets' })).toBeInTheDocument()
    // Group heading for the network group (nav.network).
    expect(screen.getByRole('heading', { name: 'Network' })).toBeInTheDocument()
    // ConnectionsSettings resolves its config async — the subtitle proves the
    // gateway side mounted past its loading pulse.
    expect(
      await screen.findByText(/Wire chat platforms and external systems/),
    ).toBeInTheDocument()
    // NetworkSettings' proxy card lead-in.
    expect(await screen.findByText(/Proxy, bypass list and custom CA/)).toBeInTheDocument()
  })

  it('PermissionsPane opens with the default-tier guidance card that jumps to General', async () => {
    render(
      wrap(
        <Routes>
          <Route path="/settings/permissions" element={<PermissionsPane />} />
          <Route path="/settings/general" element={<div data-testid="general-pane-marker">general pane</div>} />
        </Routes>,
        ['/settings/permissions'],
      ),
    )
    expect(screen.getByText('Default execution tier lives in General')).toBeInTheDocument()
    screen.getByTestId('permissions-to-general-link').click()
    await waitFor(() =>
      expect(screen.getByTestId('general-pane-marker')).toBeInTheDocument(),
    )
  })
})
