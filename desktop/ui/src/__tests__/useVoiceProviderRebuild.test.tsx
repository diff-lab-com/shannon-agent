// B1-5 P1-5: the STT provider used to be built once at first mount and
// never replaced — a cold start straight into /chat rendered ChatInput
// before the config loaded, so local-STT users silently recorded through
// the cloud provider. These tests pin the rebuild contract: config
// arriving (or changing) rebuilds the provider, except mid-capture where
// the rebuild waits for the recording to finish.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { useVoice } from '@/hooks/useVoice'
import { createVoiceProvider, type VoiceProviderConfig } from '@/lib/voice'

// Wrap (not replace) the real factory so each test can assert on the
// config the hook resolved, while jsdom's missing MediaRecorder still
// exercises the genuine stub-fallback path (F-voice-gate).
vi.mock('@/lib/voice', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/voice')>()
  return {
    ...actual,
    createVoiceProvider: vi.fn(actual.createVoiceProvider),
  }
})

interface ProbeProps {
  provider: 'cloud' | 'local'
  local?: { model: string | null; language?: string | null }
  onTranscript?: (text: string) => void
}

function VoiceProbe({ provider, local, onTranscript }: ProbeProps) {
  const v = useVoice({ provider, local, onTranscript })
  return (
    <div>
      <div data-testid="state">{v.state}</div>
      <div data-testid="supported">{v.supported ? 'yes' : 'no'}</div>
      <button onClick={() => void v.startRecording()}>start</button>
      <button onClick={() => void v.stopRecording()}>stop</button>
    </div>
  )
}

function lastBuild(): VoiceProviderConfig | undefined {
  const calls = vi.mocked(createVoiceProvider).mock.calls
  return calls[calls.length - 1]?.[0]
}

/** jsdom has neither MediaRecorder nor getUserMedia; install throwaway fakes. */
function installMediaRecorder() {
  class FakeMediaRecorder {
    static isTypeSupported() {
      return true
    }
    start() {}
    stop() {}
  }
  ;(globalThis as unknown as { MediaRecorder?: unknown }).MediaRecorder = FakeMediaRecorder
  const originalGUM = navigator.mediaDevices?.getUserMedia
  Object.defineProperty(navigator, 'mediaDevices', {
    value: { getUserMedia: vi.fn().mockResolvedValue({ getTracks: () => [{ stop: vi.fn() }] }) },
    configurable: true,
  })
  return () => {
    delete (globalThis as unknown as { MediaRecorder?: unknown }).MediaRecorder
    if (originalGUM === undefined) {
      delete (navigator as unknown as { mediaDevices?: unknown }).mediaDevices
    } else {
      Object.defineProperty(navigator, 'mediaDevices', {
        value: { getUserMedia: originalGUM },
        configurable: true,
      })
    }
  }
}

describe('useVoice — provider rebuild on config change (B1-5 P1-5)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('rebuilds as the local provider once config arrives after mount', async () => {
    // Cold start: config still loading — ChatInput resolves cloud.
    const { rerender } = render(<VoiceProbe provider="cloud" />)
    expect(lastBuild()).toMatchObject({ kind: 'remote' })
    expect(vi.mocked(createVoiceProvider)).toHaveBeenCalledTimes(1)

    // Config arrives with voice_local enabled → same mount, new provider.
    rerender(<VoiceProbe provider="local" local={{ model: 'tiny', language: 'en' }} />)
    await waitFor(() =>
      expect(lastBuild()).toMatchObject({
        kind: 'local',
        local: { model: 'tiny', language: 'en' },
      }),
    )
    expect(vi.mocked(createVoiceProvider)).toHaveBeenCalledTimes(2)
  })

  it('rebuilds when local settings change while idle', async () => {
    const { rerender } = render(
      <VoiceProbe provider="local" local={{ model: 'tiny', language: 'en' }} />,
    )
    expect(lastBuild()).toMatchObject({ kind: 'local', local: { model: 'tiny' } })

    rerender(<VoiceProbe provider="local" local={{ model: 'base', language: null }} />)
    await waitFor(() =>
      expect(lastBuild()).toMatchObject({
        kind: 'local',
        local: { model: 'base', language: null },
      }),
    )
  })

  it('defers the rebuild while recording and lands it after the capture ends', async () => {
    const onTranscript = vi.fn()
    const { rerender } = render(<VoiceProbe provider="cloud" onTranscript={onTranscript} />)
    expect(lastBuild()).toMatchObject({ kind: 'remote' })

    // Start recording on the cloud provider...
    fireEvent.click(screen.getByText('start'))
    expect(screen.getByTestId('state')).toHaveTextContent('recording')

    // ...and flip to local mid-capture: nothing is rebuilt, the state
    // is untouched (the old provider must not be aborted).
    rerender(
      <VoiceProbe provider="local" local={{ model: 'tiny', language: 'en' }} onTranscript={onTranscript} />,
    )
    expect(screen.getByTestId('state')).toHaveTextContent('recording')
    expect(vi.mocked(createVoiceProvider)).toHaveBeenCalledTimes(1)

    // Stop → the original provider still delivers its transcript...
    fireEvent.click(screen.getByText('stop'))
    await waitFor(() =>
      expect(onTranscript).toHaveBeenCalledWith(expect.stringContaining('stub transcript')),
    )
    // ...and only once back at idle does the local provider get built.
    await waitFor(() => expect(lastBuild()).toMatchObject({ kind: 'local' }))
    expect(vi.mocked(createVoiceProvider)).toHaveBeenCalledTimes(2)
    await waitFor(() => expect(screen.getByTestId('state')).toHaveTextContent('idle'))
  })

  it('recomputes `supported` after a rebuild (F-voice-gate stays honest)', () => {
    // No MediaRecorder → factory falls back to the stub → unsupported.
    const { rerender } = render(<VoiceProbe provider="cloud" />)
    expect(screen.getByTestId('supported')).toHaveTextContent('no')

    const teardown = installMediaRecorder()
    try {
      // Rebuild with capture seams present → the local provider now
      // reports supported, and the hook must surface that (a stale
      // providerRef read would keep the mic hidden forever).
      rerender(<VoiceProbe provider="local" local={{ model: null, language: null }} />)
      expect(screen.getByTestId('supported')).toHaveTextContent('yes')
      expect(lastBuild()).toMatchObject({ kind: 'local' })
    } finally {
      teardown()
    }
  })
})
