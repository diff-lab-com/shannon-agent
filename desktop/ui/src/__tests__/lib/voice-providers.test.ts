import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createVoiceProvider, defaultVoiceConfig } from '@/lib/voice/factory'
import { createRemoteProvider } from '@/lib/voice/remoteProvider'
import { transcribeAudio } from '@/lib/tauri-api'

interface FakeRecorder {
  ondataavailable: ((e: { data: Blob }) => void) | null
  onstop: (() => void) | null
  start(): void
  stop(): void
}

/**
 * Install a fake `MediaRecorder` + `navigator.mediaDevices.getUserMedia` so the
 * remote provider reports as supported. Returns the created recorder instances
 * (so a test can push audio chunks / fire onstop) and a teardown that restores
 * the originals.
 */
function installMediaRecorder(opts: { getUserMediaRejects?: boolean } = {}) {
  const originalMR = (globalThis as unknown as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder
  const originalGUM = navigator.mediaDevices?.getUserMedia
  const instances: FakeRecorder[] = []

  class FakeMediaRecorder {
    ondataavailable: ((e: { data: Blob }) => void) | null = null
    onstop: (() => void) | null = null
    constructor() {
      instances.push(this as unknown as FakeRecorder)
    }
    static isTypeSupported() {
      return true
    }
    start() {}
    stop() {
      this.onstop?.()
    }
  }
  ;(globalThis as unknown as { MediaRecorder: typeof MediaRecorder }).MediaRecorder =
    FakeMediaRecorder as unknown as typeof MediaRecorder
  const gum = opts.getUserMediaRejects
    ? vi.fn().mockRejectedValue(new Error('Permission denied'))
    : vi.fn().mockResolvedValue({ getTracks: () => [{ stop: vi.fn() }] })
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

describe('defaultVoiceConfig', () => {
  it('defaults to the cloud (remote) STT provider', () => {
    expect(defaultVoiceConfig().kind).toBe('remote')
  })
})

describe('createVoiceProvider', () => {
  beforeEach(() => {
    // No MediaRecorder by default → remote provider is unsupported.
    delete (globalThis as unknown as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder
  })

  it('returns a stub provider when kind is stub, and the stub reports itself unsupported', () => {
    const p = createVoiceProvider({ kind: 'stub' })
    expect(p.kind).toBe('stub')
    // F-voice-gate: the stub is the fallback for environments WITHOUT real
    // STT support, so it must report isSupported() === false — otherwise the
    // ChatInput mic gate can never close and users without a provider get a
    // mic that only ever emits the stub's canned transcript.
    expect(p.isSupported()).toBe(false)
  })

  it('falls back to stub when remote is unsupported (no MediaRecorder), and that stub is unsupported', () => {
    const p = createVoiceProvider({ kind: 'remote' })
    expect(p.kind).toBe('stub')
    // F-voice-gate: the fallback must surface as unsupported so
    // useVoice().supported is false and the UI hides the mic.
    expect(p.isSupported()).toBe(false)
  })

  it('returns the remote provider when MediaRecorder is available', () => {
    const { teardown } = installMediaRecorder()
    try {
      const p = createVoiceProvider({ kind: 'remote' })
      expect(p.kind).toBe('remote')
      expect(p.isSupported()).toBe(true)
    } finally {
      teardown()
    }
  })

  // P2-5e: the local provider has the same MediaRecorder-based
  // capture path as the remote one, so the factory uses the same
  // support gate. We assert the kind routes correctly without
  // exercising the actual whisper-rs round-trip (the Rust
  // integration test for that lives in the desktop crate).
  it('returns the local provider when MediaRecorder is available', () => {
    const { teardown } = installMediaRecorder()
    try {
      const p = createVoiceProvider({
        kind: 'local',
        local: { model: 'base', language: 'en' },
      })
      expect(p.kind).toBe('local')
      expect(p.isSupported()).toBe(true)
    } finally {
      teardown()
    }
  })

  it('falls back to stub when local is unsupported (no MediaRecorder), and that stub is unsupported', () => {
    const p = createVoiceProvider({ kind: 'local' })
    expect(p.kind).toBe('stub')
    expect(p.isSupported()).toBe(false)
  })
})

// F-voice-gate: the factory's support surface must be honest. A provider is
// "supported" only when BOTH real capture seams exist (`MediaRecorder` global
// + `navigator.mediaDevices.getUserMedia`), and the stub fallback — whatever
// route led to it — always reports unsupported so `useVoice().supported` is
// false and ChatInput hides the mic instead of offering a recording that can
// only ever emit the stub's canned transcript.
describe('factory isSupported gate (F-voice-gate)', () => {
  interface Seam {
    recorder: boolean
    mediaDevices: boolean
  }

  /** Install/remove exactly the two capture seams isSupported() probes. */
  function installSeams(seam: Seam): () => void {
    const g = globalThis as unknown as { MediaRecorder?: unknown }
    const originalMR = g.MediaRecorder
    const originalDesc = Object.getOwnPropertyDescriptor(navigator, 'mediaDevices')

    if (seam.recorder) {
      g.MediaRecorder = class {
        static isTypeSupported() {
          return true
        }
      }
    } else {
      delete g.MediaRecorder
    }
    Object.defineProperty(
      navigator,
      'mediaDevices',
      seam.mediaDevices
        ? { value: { getUserMedia: vi.fn() }, configurable: true }
        : { get: () => undefined, configurable: true },
    )

    return () => {
      if (originalMR === undefined) delete g.MediaRecorder
      else g.MediaRecorder = originalMR
      if (originalDesc) Object.defineProperty(navigator, 'mediaDevices', originalDesc)
      else delete (navigator as unknown as { mediaDevices?: unknown }).mediaDevices
    }
  }

  const cases: Array<{
    name: string
    kind: 'remote' | 'local' | 'stub'
    seam: Seam
    expectedKind: 'remote' | 'local' | 'stub'
    expectedSupported: boolean
  }> = [
    { name: 'remote with both seams → remote, supported', kind: 'remote', seam: { recorder: true, mediaDevices: true }, expectedKind: 'remote', expectedSupported: true },
    { name: 'remote without getUserMedia → stub fallback, unsupported', kind: 'remote', seam: { recorder: true, mediaDevices: false }, expectedKind: 'stub', expectedSupported: false },
    { name: 'remote without MediaRecorder → stub fallback, unsupported', kind: 'remote', seam: { recorder: false, mediaDevices: true }, expectedKind: 'stub', expectedSupported: false },
    { name: 'remote with no seams → stub fallback, unsupported', kind: 'remote', seam: { recorder: false, mediaDevices: false }, expectedKind: 'stub', expectedSupported: false },
    { name: 'local with both seams → local, supported', kind: 'local', seam: { recorder: true, mediaDevices: true }, expectedKind: 'local', expectedSupported: true },
    { name: 'local without getUserMedia → stub fallback, unsupported', kind: 'local', seam: { recorder: true, mediaDevices: false }, expectedKind: 'stub', expectedSupported: false },
    { name: 'local without MediaRecorder → stub fallback, unsupported', kind: 'local', seam: { recorder: false, mediaDevices: true }, expectedKind: 'stub', expectedSupported: false },
    { name: 'explicit stub reports unsupported even with both seams', kind: 'stub', seam: { recorder: true, mediaDevices: true }, expectedKind: 'stub', expectedSupported: false },
    { name: 'explicit stub reports unsupported with no seams', kind: 'stub', seam: { recorder: false, mediaDevices: false }, expectedKind: 'stub', expectedSupported: false },
  ]

  it.each(cases)('$name', ({ kind, seam, expectedKind, expectedSupported }) => {
    const restore = installSeams(seam)
    try {
      const p = createVoiceProvider({ kind })
      expect(p.kind).toBe(expectedKind)
      expect(p.isSupported()).toBe(expectedSupported)
    } finally {
      restore()
    }
  })
})

describe('stub provider', () => {
  it('emits one partial on start and a final transcript on stop', async () => {
    const p = createVoiceProvider({ kind: 'stub' })
    const onResult = vi.fn()
    const onEnd = vi.fn()
    await p.start({ onResult, onError: vi.fn(), onEnd })
    expect(onResult).toHaveBeenCalledTimes(1)
    expect(onResult.mock.calls[0][0]).toMatchObject({ isFinal: false })
    await p.stop()
    const finalCall = onResult.mock.calls.find((c) => (c[0] as { transcript?: string }).transcript)
    expect(finalCall).toBeTruthy()
    expect((finalCall![0] as { transcript: string }).transcript).toContain('stub transcript')
    expect(onEnd).toHaveBeenCalled()
  })

  it('abort prevents further emissions', async () => {
    const p = createVoiceProvider({ kind: 'stub' })
    const onResult = vi.fn()
    await p.start({ onResult, onError: vi.fn() })
    p.abort()
    onResult.mockClear()
    await p.stop()
    expect(onResult).not.toHaveBeenCalled()
  })
})

describe('remote provider', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(transcribeAudio).mockReset()
  })
  afterEach(() => {
    vi.mocked(transcribeAudio).mockReset()
  })

  it('transcribes the captured audio via the backend', async () => {
    vi.mocked(transcribeAudio).mockResolvedValue({ text: 'hello world' })
    const { teardown, instances } = installMediaRecorder()
    try {
      const p = createRemoteProvider({ kind: 'remote' })
      const onResult = vi.fn()
      const onEnd = vi.fn()
      await p.start({ onResult, onError: vi.fn(), onEnd })
      // Push a recorded chunk, then stop → onstop → flush → transcribe.
      instances[0].ondataavailable!({ data: new Blob(['audio']) })
      await p.stop()
      await vi.waitFor(() => expect(transcribeAudio).toHaveBeenCalledTimes(1))
      expect(transcribeAudio).toHaveBeenCalledWith(expect.any(String), 'audio/webm')
      await vi.waitFor(() => expect(onResult).toHaveBeenCalledWith({ transcript: 'hello world' }))
      expect(onEnd).toHaveBeenCalled()
    } finally {
      teardown()
    }
  })

  it('maps a backend STT_NOT_CONFIGURED rejection to the not-configured code', async () => {
    vi.mocked(transcribeAudio).mockRejectedValue(
      'STT_NOT_CONFIGURED: configure a speech-to-text provider in Settings',
    )
    const { teardown, instances } = installMediaRecorder()
    try {
      const p = createRemoteProvider({ kind: 'remote' })
      const onError = vi.fn()
      await p.start({ onResult: vi.fn(), onError })
      instances[0].ondataavailable!({ data: new Blob(['audio']) })
      await p.stop()
      await vi.waitFor(() => expect(onError).toHaveBeenCalled())
      expect(onError.mock.calls[0][0].code).toBe('not-configured')
      // The machine prefix is stripped from the surfaced message.
      expect(onError.mock.calls[0][0].message).not.toContain('STT_NOT_CONFIGURED')
    } finally {
      teardown()
    }
  })

  it('reports mic-denied when getUserMedia rejects', async () => {
    const { teardown } = installMediaRecorder({ getUserMediaRejects: true })
    try {
      const p = createRemoteProvider({ kind: 'remote' })
      const onError = vi.fn()
      await p.start({ onResult: vi.fn(), onError })
      expect(onError).toHaveBeenCalledTimes(1)
      expect(onError.mock.calls[0][0].code).toBe('mic-denied')
      expect(transcribeAudio).not.toHaveBeenCalled()
    } finally {
      teardown()
    }
  })
})
