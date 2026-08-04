import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createLocalProvider } from '@/lib/voice/localProvider'
import { transcribeAudioLocalBase64 } from '@/lib/tauri-api'

interface FakeRecorder {
  ondataavailable: ((e: { data: Blob }) => void) | null
  onstop: (() => void) | null
  start(): void
  stop(): void
}

/**
 * Mirror of the MediaRecorder fake in `voice-providers.test.ts`.
 * Kept inline (not exported) so the two test files are
 * independent — a future split into a shared helper is fine
 * but not required.
 */
function installMediaRecorder() {
  const originalMR = (globalThis as unknown as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder
  const originalGUM = navigator.mediaDevices?.getUserMedia
  const instances: FakeRecorder[] = []

  class FakeMediaRecorder {
    ondataavailable: ((e: { data: Blob }) => void) | null = null
    onstop: (() => void) | null = null
    constructor() {
      instances.push(this as unknown as FakeRecorder)
    }
    static isTypeSupported(_mime: string) {
      // Always claim WAV support; the provider picks
      // `audio/wav` first, then webm/ogg.
      return true
    }
    start() {}
    stop() {
      this.onstop?.()
    }
  }
  ;(globalThis as unknown as { MediaRecorder: typeof MediaRecorder }).MediaRecorder =
    FakeMediaRecorder as unknown as typeof MediaRecorder
  const gum = vi.fn().mockResolvedValue({ getTracks: () => [{ stop: vi.fn() }] })
  Object.defineProperty(navigator, 'mediaDevices', {
    value: { getUserMedia: gum },
    configurable: true,
  })

  const teardown = () => {
    if (originalMR === undefined) {
      delete (globalThis as unknown as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder
    } else {
      ;(globalThis as unknown as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder = originalMR
    }
    if (originalGUM === undefined) {
      delete (navigator as unknown as { mediaDevices?: unknown }).mediaDevices
    } else {
      Object.defineProperty(navigator, 'mediaDevices', {
        value: { getUserMedia: originalGUM },
        configurable: true,
      })
    }
  }
  return { teardown, instances }
}

describe('localProvider', () => {
  beforeEach(() => {
    vi.mocked(transcribeAudioLocalBase64).mockReset()
  })

  afterEach(() => {
    vi.mocked(transcribeAudioLocalBase64).mockReset()
  })

  it('reports itself as supported when MediaRecorder is available', () => {
    const { teardown } = installMediaRecorder()
    try {
      const p = createLocalProvider({
        kind: 'local',
        local: { model: 'base', language: 'en' },
      })
      expect(p.kind).toBe('local')
      expect(p.isSupported()).toBe(true)
    } finally {
      teardown()
    }
  })

  it('forwards the recording to transcribeAudioLocalBase64 with the configured model', async () => {
    vi.mocked(transcribeAudioLocalBase64).mockResolvedValue({ text: 'local transcript' })
    const { teardown, instances } = installMediaRecorder()
    try {
      const p = createLocalProvider({
        kind: 'local',
        local: { model: 'small', language: 'zh' },
      })
      const onResult = vi.fn()
      const onEnd = vi.fn()
      await p.start({ onResult, onError: vi.fn(), onEnd })
      instances[0].ondataavailable!({ data: new Blob(['hello']) })
      await p.stop()
      await vi.waitFor(() =>
        expect(transcribeAudioLocalBase64).toHaveBeenCalledTimes(1),
      )
      expect(transcribeAudioLocalBase64).toHaveBeenCalledWith(
        expect.any(String),
        'audio/wav',
        'small',
        'zh',
      )
      await vi.waitFor(() =>
        expect(onResult).toHaveBeenCalledWith({ transcript: 'local transcript' }),
      )
      expect(onEnd).toHaveBeenCalled()
    } finally {
      teardown()
    }
  })

  it('maps STT_MODEL_NOT_FOUND to the model-not-found code', async () => {
    vi.mocked(transcribeAudioLocalBase64).mockRejectedValue(
      'STT_MODEL_NOT_FOUND: model not on disk',
    )
    const { teardown, instances } = installMediaRecorder()
    try {
      const p = createLocalProvider({
        kind: 'local',
        local: { model: 'base' },
      })
      const onError = vi.fn()
      await p.start({ onResult: vi.fn(), onError })
      instances[0].ondataavailable!({ data: new Blob(['audio']) })
      await p.stop()
      await vi.waitFor(() => expect(onError).toHaveBeenCalled())
      expect(onError.mock.calls[0][0].code).toBe('model-not-found')
      expect(onError.mock.calls[0][0].message).not.toContain('STT_MODEL_NOT_FOUND')
    } finally {
      teardown()
    }
  })

  it('maps STT_INFERENCE_FAILED to the inference-failed code', async () => {
    vi.mocked(transcribeAudioLocalBase64).mockRejectedValue(
      'STT_INFERENCE_FAILED: whisper context: bad model',
    )
    const { teardown, instances } = installMediaRecorder()
    try {
      const p = createLocalProvider({
        kind: 'local',
        local: { model: 'base' },
      })
      const onError = vi.fn()
      await p.start({ onResult: vi.fn(), onError })
      instances[0].ondataavailable!({ data: new Blob(['audio']) })
      await p.stop()
      await vi.waitFor(() => expect(onError).toHaveBeenCalled())
      expect(onError.mock.calls[0][0].code).toBe('inference-failed')
    } finally {
      teardown()
    }
  })

  it('maps STT_AUDIO_INVALID to the audio-invalid code', async () => {
    vi.mocked(transcribeAudioLocalBase64).mockRejectedValue(
      'STT_AUDIO_INVALID: only accepts audio/wav',
    )
    const { teardown, instances } = installMediaRecorder()
    try {
      const p = createLocalProvider({
        kind: 'local',
        local: { model: 'base' },
      })
      const onError = vi.fn()
      await p.start({ onResult: vi.fn(), onError })
      instances[0].ondataavailable!({ data: new Blob(['audio']) })
      await p.stop()
      await vi.waitFor(() => expect(onError).toHaveBeenCalled())
      expect(onError.mock.calls[0][0].code).toBe('audio-invalid')
    } finally {
      teardown()
    }
  })
})
