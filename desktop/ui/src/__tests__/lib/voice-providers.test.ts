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

  it('returns a stub provider when kind is stub', () => {
    const p = createVoiceProvider({ kind: 'stub' })
    expect(p.kind).toBe('stub')
    expect(p.isSupported()).toBe(true)
  })

  it('falls back to stub when remote is unsupported (no MediaRecorder)', () => {
    const p = createVoiceProvider({ kind: 'remote' })
    expect(p.kind).toBe('stub')
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
