import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import PersonaPackSettings from '@/components/settings/PersonaPackSettings'
import { save as saveDialog, open as openDialog } from '@tauri-apps/plugin-dialog'
import * as api from '@/lib/tauri-api'

const COUNTS = { skills: 2, commands: 1, memories: 3, routines: 2, profiles: 1, persona: 1 }
const ZERO_COUNTS = { skills: 0, commands: 0, memories: 0, routines: 0, profiles: 0, persona: 0 }

describe('PersonaPackSettings', () => {
  beforeEach(() => {
    vi.mocked(saveDialog).mockReset().mockResolvedValue(null)
    vi.mocked(openDialog).mockReset().mockResolvedValue(null)
    vi.mocked(api.personaPackExport).mockReset().mockResolvedValue({
      path: '/tmp/shannon-pack.tar.gz',
      counts: ZERO_COUNTS,
      stripped: 0,
    })
    vi.mocked(api.personaPackInspect).mockReset().mockResolvedValue({
      version: 1,
      counts: ZERO_COUNTS,
      createdAtMs: 0,
      generator: 'shannon-test',
    })
    vi.mocked(api.personaPackImport).mockReset().mockResolvedValue({
      imported: ZERO_COUNTS,
      skipped: ZERO_COUNTS,
      failed: [],
    })
  })

  it('renders the section with export and import blocks', () => {
    render(<PersonaPackSettings />)
    expect(screen.getByRole('heading', { name: 'Configuration pack' })).toBeInTheDocument()
    expect(screen.getByText('Export pack')).toBeInTheDocument()
    expect(screen.getByText('Import pack')).toBeInTheDocument()
  })

  it('renders all six include checkboxes and toggles them', () => {
    render(<PersonaPackSettings />)
    for (const key of ['skills', 'commands', 'memory', 'routines', 'profiles', 'persona']) {
      const box = screen.getByTestId(`persona-pack-include-${key}`) as HTMLInputElement
      expect(box).not.toBeChecked()
      fireEvent.click(box)
      expect(box).toBeChecked()
    }
  })

  it('export flow: save dialog + counts chips + stripped-secret note', async () => {
    vi.mocked(saveDialog).mockResolvedValueOnce('/tmp/pack.tar.gz')
    vi.mocked(api.personaPackExport).mockResolvedValueOnce({
      path: '/tmp/pack.tar.gz',
      counts: COUNTS,
      stripped: 3,
    })
    render(<PersonaPackSettings />)
    fireEvent.click(screen.getByTestId('persona-pack-include-skills'))
    fireEvent.click(screen.getByTestId('persona-pack-export'))
    await waitFor(() =>
      expect(screen.getByTestId('persona-pack-export-result')).toBeInTheDocument(),
    )
    expect(api.personaPackExport).toHaveBeenCalledWith(
      '/tmp/pack.tar.gz',
      expect.objectContaining({ skills: true, commands: false }),
    )
    expect(screen.getByTestId('persona-pack-export-count-skills')).toHaveTextContent('Skills: 2')
    expect(screen.getByTestId('persona-pack-stripped-note')).toHaveTextContent(
      /3 secrets were stripped/,
    )
  })

  it('export flow: zero stripped secrets shows the clean note', async () => {
    vi.mocked(saveDialog).mockResolvedValueOnce('/tmp/pack.tar.gz')
    render(<PersonaPackSettings />)
    fireEvent.click(screen.getByTestId('persona-pack-export'))
    await waitFor(() =>
      expect(screen.getByTestId('persona-pack-export-result')).toBeInTheDocument(),
    )
    expect(screen.queryByTestId('persona-pack-stripped-note')).not.toBeInTheDocument()
    expect(screen.getByText('No secrets found — nothing was stripped.')).toBeInTheDocument()
  })

  it('export flow: cancelled dialog does not call the backend', async () => {
    render(<PersonaPackSettings />)
    fireEvent.click(screen.getByTestId('persona-pack-export'))
    await waitFor(() => expect(saveDialog).toHaveBeenCalled())
    expect(api.personaPackExport).not.toHaveBeenCalled()
  })

  it('import flow: pick → inspect preview table → conflict radio → import result', async () => {
    vi.mocked(openDialog).mockResolvedValueOnce('/tmp/incoming.tar.gz')
    vi.mocked(api.personaPackInspect).mockResolvedValueOnce({
      version: 1,
      counts: COUNTS,
      createdAtMs: 1720000000000,
      generator: 'shannon-0.11.0',
    })
    vi.mocked(api.personaPackImport).mockResolvedValueOnce({
      imported: COUNTS,
      skipped: ZERO_COUNTS,
      failed: [{ item: 'skills/leaky/SKILL.md', error: 'cannot write' }],
    })
    render(<PersonaPackSettings />)
    fireEvent.click(screen.getByTestId('persona-pack-pick'))

    // Preview table renders after inspect.
    await waitFor(() => expect(screen.getByTestId('persona-pack-preview')).toBeInTheDocument())
    expect(api.personaPackInspect).toHaveBeenCalledWith('/tmp/incoming.tar.gz')
    expect(screen.getByTestId('persona-pack-preview-memory')).toHaveTextContent('3')
    expect(screen.getByText(/shannon-0.11.0/)).toBeInTheDocument()
    expect(
      screen.getByText('When a target already exists with different content'),
    ).toBeInTheDocument()

    // Choose overwrite, then import.
    fireEvent.click(screen.getByTestId('persona-pack-conflict-overwrite'))
    fireEvent.click(screen.getByTestId('persona-pack-import'))
    await waitFor(() =>
      expect(screen.getByTestId('persona-pack-import-result')).toBeInTheDocument(),
    )
    expect(api.personaPackImport).toHaveBeenCalledWith(
      '/tmp/incoming.tar.gz',
      'overwrite',
      expect.objectContaining({ skills: false }),
    )
    expect(screen.getByTestId('persona-pack-imported-skills')).toHaveTextContent('Skills: 2 / 0')
    expect(screen.getByTestId('persona-pack-failures')).toHaveTextContent('cannot write')
  })

  it('import flow: failed inspect keeps the preview hidden', async () => {
    vi.mocked(openDialog).mockResolvedValueOnce('/tmp/broken.tar.gz')
    vi.mocked(api.personaPackInspect).mockRejectedValueOnce(
      new Error('corrupt pack: manifest.json missing'),
    )
    render(<PersonaPackSettings />)
    fireEvent.click(screen.getByTestId('persona-pack-pick'))
    await waitFor(() => expect(api.personaPackInspect).toHaveBeenCalled())
    expect(screen.queryByTestId('persona-pack-preview')).not.toBeInTheDocument()
  })

  it('a11y: export button is labelled and include checkboxes are grouped', () => {
    render(<PersonaPackSettings />)
    expect(screen.getByLabelText('Export configuration pack to a file')).toBeInTheDocument()
    expect(screen.getByText('Include in pack')).toBeInTheDocument()
    for (const key of ['skills', 'commands', 'memory', 'routines', 'profiles', 'persona']) {
      const box = screen.getByTestId(`persona-pack-include-${key}`)
      expect(box).toHaveAttribute('id', `persona-pack-include-${key}`)
    }
  })
})
