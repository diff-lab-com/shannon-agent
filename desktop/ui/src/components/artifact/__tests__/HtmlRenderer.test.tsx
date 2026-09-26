import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, waitFor } from '@testing-library/react'
import { HtmlRenderer } from '../HtmlRenderer'

/**
 * 2026-09-26 round2 §5-1 A — interactive HTML via the artifact:// custom
 * protocol. Security-critical contract:
 *   * interactive mode loads the URL returned by the Rust registry into a
 *     sandboxed iframe — sandbox EXACTLY "allow-scripts allow-forms"
 *     (never allow-same-origin / allow-top-navigation / allow-modals /
 *     allow-popups);
 *   * the registry entry is released on unmount and on source change;
 *   * registration failure falls back to the byte-identical static srcDoc
 *     path (empty sandbox), and reports it via onRegistrationFailed.
 */

const registerInteractiveHtml = vi.hoisted(() => vi.fn())
const unregisterInteractiveArtifact = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  registerInteractiveHtml,
  unregisterInteractiveArtifact,
}))

const mockedRegister = vi.mocked(registerInteractiveHtml)
const mockedUnregister = vi.mocked(unregisterInteractiveArtifact)

beforeEach(() => {
  mockedRegister.mockReset()
  mockedUnregister.mockReset()
  mockedRegister.mockResolvedValue({ id: 'art-1', url: 'artifact://localhost/art-1' })
  mockedUnregister.mockResolvedValue(undefined)
})

afterEach(() => {
  cleanup()
})

describe('HtmlRenderer static mode (default)', () => {
  it('keeps the srcDoc + empty-sandbox posture unchanged', () => {
    const { container } = render(<HtmlRenderer source="<p>static</p>" />)
    const iframe = container.querySelector('iframe')!
    expect(iframe.getAttribute('sandbox')).toBe('')
    const srcDoc = iframe.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain('<p>static</p>')
    expect(srcDoc).toContain("default-src 'none'")
    expect(iframe.getAttribute('src')).toBeNull()
    expect(mockedRegister).not.toHaveBeenCalled()
  })

  it('interactive=false never touches the registry', () => {
    render(<HtmlRenderer source="<p>static</p>" interactive={false} />)
    expect(mockedRegister).not.toHaveBeenCalled()
  })
})

describe('HtmlRenderer interactive mode', () => {
  it('loads the registered URL in a sandbox with exactly allow-scripts allow-forms', async () => {
    const { container } = render(<HtmlRenderer source="<p>live</p>" interactive />)
    const iframe = await waitFor(() => {
      const el = container.querySelector('iframe[src]')
      expect(el).toBeTruthy()
      return el!
    })
    expect(iframe.getAttribute('src')).toBe('artifact://localhost/art-1')
    // The whole point of §5-1 A: scripts run, but the document stays an
    // opaque origin with no navigation/modals/popups escapes.
    expect(iframe.getAttribute('sandbox')).toBe('allow-scripts allow-forms')
    expect(iframe.getAttribute('sandbox')!.split(' ')).not.toContain('allow-same-origin')
    // While interactive the static srcdoc is not rendered side-by-side.
    expect(iframe.getAttribute('srcdoc')).toBeNull()
    expect(mockedRegister).toHaveBeenCalledWith('<p>live</p>')
  })

  it('registers on source change and unregisters the previous id', async () => {
    const { rerender } = render(<HtmlRenderer source="<p>v1</p>" interactive />)
    await waitFor(() => {
      expect(mockedRegister).toHaveBeenCalledWith('<p>v1</p>')
    })
    mockedRegister.mockResolvedValueOnce({ id: 'art-2', url: 'artifact://localhost/art-2' })
    rerender(<HtmlRenderer source="<p>v2</p>" interactive />)
    await waitFor(() => {
      expect(mockedRegister).toHaveBeenCalledWith('<p>v2</p>')
    })
    await waitFor(() => {
      expect(mockedUnregister).toHaveBeenCalledWith('art-1')
    })
    expect(mockedUnregister).not.toHaveBeenCalledWith('art-2')
  })

  it('unregisters on unmount', async () => {
    const { unmount } = render(<HtmlRenderer source="<p>bye</p>" interactive />)
    await waitFor(() => {
      expect(mockedRegister).toHaveBeenCalled()
    })
    unmount()
    await waitFor(() => {
      expect(mockedUnregister).toHaveBeenCalledWith('art-1')
    })
  })

  it('falls back to the static srcDoc when registration rejects', async () => {
    mockedRegister.mockRejectedValue(new Error('command unavailable'))
    const onFailed = vi.fn()
    const { container } = render(
      <HtmlRenderer source="<p>fallback</p>" interactive onRegistrationFailed={onFailed} />,
    )
    await waitFor(() => {
      expect(onFailed).toHaveBeenCalledTimes(1)
    })
    const iframe = container.querySelector('iframe')!
    expect(iframe.getAttribute('sandbox')).toBe('')
    const srcDoc = iframe.getAttribute('srcdoc') ?? ''
    expect(srcDoc).toContain('<p>fallback</p>')
    expect(srcDoc).toContain("default-src 'none'")
    expect(iframe.getAttribute('src')).toBeNull()
    expect(mockedUnregister).not.toHaveBeenCalled()
  })

  it('renders the static frame while registration is in flight (no blank flash)', () => {
    mockedRegister.mockReturnValue(new Promise(() => {}))
    const { container } = render(<HtmlRenderer source="<p>pending</p>" interactive />)
    const iframe = container.querySelector('iframe')!
    // Still the harmless static document until the live URL arrives.
    expect(iframe.getAttribute('sandbox')).toBe('')
    expect(iframe.getAttribute('srcdoc')).toContain('<p>pending</p>')
    expect(iframe.getAttribute('src')).toBeNull()
  })

  it('drops a late registration result after unmount', async () => {
    let resolveLate!: (v: { id: string; url: string }) => void
    mockedRegister.mockReturnValueOnce(new Promise(resolve => { resolveLate = resolve }))
    const { unmount } = render(<HtmlRenderer source="<p>late</p>" interactive />)
    unmount()
    resolveLate({ id: 'art-late', url: 'artifact://localhost/art-late' })
    await waitFor(() => {
      // The fresh registration is released immediately — nothing leaks.
      expect(mockedUnregister).toHaveBeenCalledWith('art-late')
    })
  })
})
