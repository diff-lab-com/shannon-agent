import { describe, it, expect, vi } from 'vitest'
import { render, screen, waitFor, within } from '@testing-library/react'

import RemotesSettings from '@/components/settings/RemotesSettings'
import * as api from '@/lib/tauri-api'

// The global setup auto-wraps each render() in I18nProvider (default locale
// en) and seeds the @/lib/tauri-api factory mock with remote* defaults —
// same convention as ConnectionsSettings.test.tsx.

describe('RemotesSettings', () => {
  it('renders the section title', async () => {
    render(<RemotesSettings />)
    await waitFor(() => expect(screen.getByText('Remote targets')).toBeInTheDocument())
  })

  it('renders the empty state when no targets are saved', async () => {
    vi.spyOn(api, 'remoteListTargets').mockResolvedValueOnce([])
    render(<RemotesSettings />)
    await waitFor(() =>
      expect(screen.getByTestId('remotes-empty')).toBeInTheDocument(),
    )
    expect(screen.getByText('No remote targets yet')).toBeInTheDocument()
    expect(screen.getByTestId('remotes-empty-add')).toBeInTheDocument()
  })

  it('lists saved targets with kind and workspace details', async () => {
    render(<RemotesSettings />)
    await waitFor(() =>
      expect(screen.getByTestId('remotes-target-build-box')).toBeInTheDocument(),
    )
    const row = within(screen.getByTestId('remotes-target-build-box'))
    expect(row.getByText('build-box')).toBeInTheDocument()
    expect(row.getByText('ssh')).toBeInTheDocument()
    expect(row.getByText('build-box · /home/ed/proj')).toBeInTheDocument()
  })

  it('probes connectivity when Test is clicked', async () => {
    const spy = vi.spyOn(api, 'remoteTestTarget').mockResolvedValueOnce({
      ok: true,
      platform: 'Linux',
      home: '/home/ed',
      bashAvailable: true,
      workspaceExists: true,
      latencyMs: 7,
      error: null,
    })
    render(<RemotesSettings />)
    await waitFor(() =>
      expect(screen.getByTestId('remotes-test-build-box')).toBeInTheDocument(),
    )
    fireEvent_click(screen.getByTestId('remotes-test-build-box'))
    await waitFor(() => expect(spy).toHaveBeenCalledWith('build-box'))
    await waitFor(() =>
      expect(screen.getByTestId('remotes-health-build-box')).toHaveTextContent(
        'Linux · 7ms',
      ),
    )
  })

  it('submits the add dialog through remoteAddTarget', async () => {
    const spy = vi.spyOn(api, 'remoteAddTarget').mockResolvedValue(undefined)
    render(<RemotesSettings />)
    await waitFor(() => expect(screen.getByTestId('remotes-add')).toBeInTheDocument())
    fireEvent_click(screen.getByTestId('remotes-add'))
    fireEvent_change(screen.getByTestId('remotes-dialog-kind'), 'docker')
    fireEvent_change(screen.getByTestId('remotes-dialog-name'), 'ci')
    fireEvent_change(screen.getByTestId('remotes-dialog-detail'), 'shannon-ci')
    fireEvent_change(screen.getByTestId('remotes-dialog-workspace'), '/workspace')
    expect(screen.getByTestId('remotes-dialog-submit')).not.toBeDisabled()
    fireEvent_click(screen.getByTestId('remotes-dialog-submit'))
    await waitFor(() =>
      expect(spy).toHaveBeenCalledWith(
        expect.objectContaining({
          name: 'ci',
          kind: 'docker',
          container: 'shannon-ci',
          workspaceDir: '/workspace',
        }),
      ),
    )
  })

  it('removes a target only after confirmation', async () => {
    const spy = vi.spyOn(api, 'remoteRemoveTarget').mockResolvedValue(undefined)
    render(<RemotesSettings />)
    await waitFor(() =>
      expect(screen.getByTestId('remotes-remove-build-box')).toBeInTheDocument(),
    )
    fireEvent_click(screen.getByTestId('remotes-remove-build-box'))
    // Confirm inside the dialog (two delete buttons exist: row + confirm).
    const dialog = await waitFor(() => {
      const dialogs = screen.getAllByRole('alertdialog')
      expect(dialogs.length).toBeGreaterThan(0)
      return dialogs[dialogs.length - 1]
    })
    const confirm = within(dialog).getAllByRole('button').find((b) =>
      b.textContent?.includes('Remove'),
    )
    expect(confirm).toBeTruthy()
    fireEvent_click(confirm as HTMLElement)
    await waitFor(() => expect(spy).toHaveBeenCalledWith('build-box'))
  })
})

// Tiny helpers: the global @testing-library/react mock wraps fireEvent in
// act; re-imported here for brevity.
import { fireEvent } from '@testing-library/react'

function fireEvent_click(el: HTMLElement): void {
  fireEvent.click(el)
}

function fireEvent_change(el: HTMLElement, value: string): void {
  fireEvent.change(el, { target: { value } })
}
