import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createVoiceProvider, defaultVoiceConfig } from '@/lib/voice/factory'
import type { VoiceProviderConfig } from '@/lib/voice/types'

describe('defaultVoiceConfig', () => {
  afterEach(() => {
    delete (window as unknown as { SpeechRecognition?: unknown }).SpeechRecognition
    delete (window as unknown as { webkitSpeechRecognition?: unknown }).webkitSpeechRecognition
  })

  it('returns webspeech when window.SpeechRecognition exists', () => {
    ;(window as unknown as { SpeechRecognition?: unknown }).SpeechRecognition = function MockSpeechRecognition() {}
    expect(defaultVoiceConfig().kind).toBe('webspeech')
  })

  it('returns webspeech when window.webkitSpeechRecognition exists', () => {
    ;(window as unknown as { webkitSpeechRecognition?: unknown }).webkitSpeechRecognition = function MockSpeechRecognition() {}
    expect(defaultVoiceConfig().kind).toBe('webspeech')
  })

  it('falls back to stub when neither is present', () => {
    expect(defaultVoiceConfig().kind).toBe('stub')
  })
})

describe('createVoiceProvider', () => {
  beforeEach(() => {
    delete (window as unknown as { SpeechRecognition?: unknown }).SpeechRecognition
    delete (window as unknown as { webkitSpeechRecognition?: unknown }).webkitSpeechRecognition
  })

  it('returns stub provider when kind is stub', () => {
    const p = createVoiceProvider({ kind: 'stub' })
    expect(p.kind).toBe('stub')
    expect(p.isSupported()).toBe(true)
  })

  it('falls back to stub when webspeech is unsupported', () => {
    const p = createVoiceProvider({ kind: 'webspeech' })
    expect(p.kind).toBe('stub')
  })

  it('returns webspeech provider when supported', () => {
    ;(window as unknown as { SpeechRecognition?: unknown }).SpeechRecognition = function MockSpeechRecognition() {}
    const p = createVoiceProvider({ kind: 'webspeech' })
    expect(p.kind).toBe('webspeech')
    expect(p.isSupported()).toBe(true)
  })

  it('falls back to stub when remote is unsupported (no MediaRecorder)', () => {
    const p = createVoiceProvider({ kind: 'remote', remoteEndpoint: 'https://example.com/stt' })
    expect(p.kind).toBe('stub')
  })

  it('remote provider reports no-endpoint error when endpoint missing and runtime supports it', async () => {
    const originalMR = (globalThis as unknown as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder
    const originalGUM = navigator.mediaDevices?.getUserMedia
    // Force MediaRecorder + getUserMedia to exist so isSupported() returns true.
    ;(globalThis as unknown as { MediaRecorder: typeof MediaRecorder }).MediaRecorder = class {
      static isTypeSupported() { return true }
      start() {} stop() {}
      ondataavailable = null as unknown
      onstop = null as unknown
    } as unknown as typeof MediaRecorder
    Object.defineProperty(navigator, 'mediaDevices', {
      value: { getUserMedia: vi.fn().mockResolvedValue({} as MediaStream) },
      configurable: true,
    })
    try {
      const p = createVoiceProvider({ kind: 'remote' })
      expect(p.kind).toBe('remote')
      const onError = vi.fn()
      await p.start({ onResult: vi.fn(), onError })
      expect(onError).toHaveBeenCalledTimes(1)
      expect(onError.mock.calls[0][0].code).toBe('no-endpoint')
    } finally {
      if (originalMR !== undefined) {
        ;(globalThis as unknown as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder = originalMR
      } else {
        delete (globalThis as unknown as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder
      }
      if (originalGUM !== undefined) {
        Object.defineProperty(navigator, 'mediaDevices', {
          value: { getUserMedia: originalGUM },
          configurable: true,
        })
      }
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
    const finalCall = onResult.mock.calls.find(c => (c[0] as { transcript?: string }).transcript)
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

describe('webspeech provider', () => {
  function installMock() {
    const handles: Array<{
      lang: string
      continuous: boolean
      interimResults: boolean
      maxAlternatives: number
      onresult: ((e: unknown) => void) | null
      onerror: ((e: unknown) => void) | null
      onend: (() => void) | null
      start: () => void
      stop: () => void
      abort: () => void
    }> = []
    function Ctor() {
      const h = {
        lang: '',
        continuous: false,
        interimResults: false,
        maxAlternatives: 1,
        onresult: null as ((e: unknown) => void) | null,
        onerror: null as ((e: unknown) => void) | null,
        onend: null as (() => void) | null,
        start: vi.fn(),
        stop: vi.fn(),
        abort: vi.fn(),
      }
      handles.push(h)
      return h
    }
    ;(window as unknown as { SpeechRecognition?: unknown }).SpeechRecognition = Ctor as unknown
    return handles
  }

  beforeEach(() => {
    delete (window as unknown as { SpeechRecognition?: unknown }).SpeechRecognition
  })

  it('reports supported and starts the engine', async () => {
    const handles = installMock()
    const config: VoiceProviderConfig = { kind: 'webspeech', lang: 'en-US' }
    const p = createVoiceProvider(config)
    expect(p.kind).toBe('webspeech')
    expect(p.isSupported()).toBe(true)

    const onResult = vi.fn()
    await p.start({ onResult, onError: vi.fn() })
    expect(handles).toHaveLength(1)
    expect(handles[0].start).toHaveBeenCalled()
    expect(handles[0].lang).toBe('en-US')
    expect(handles[0].continuous).toBe(true)
    expect(handles[0].interimResults).toBe(true)
  })

  it('emits a final transcript via onresult when a final result arrives', async () => {
    const handles = installMock()
    const onResult = vi.fn()
    const p = createVoiceProvider({ kind: 'webspeech' })
    await p.start({ onResult, onError: vi.fn() })
    handles[0].onresult?.({
      resultIndex: 0,
      results: [{ 0: { transcript: 'hello world' }, isFinal: true }],
    })
    expect(onResult).toHaveBeenCalledWith({ transcript: 'hello world' })
  })

  it('forwards interim partials via onresult', async () => {
    const handles = installMock()
    const onResult = vi.fn()
    const p = createVoiceProvider({ kind: 'webspeech' })
    await p.start({ onResult, onError: vi.fn() })
    handles[0].onresult?.({
      resultIndex: 0,
      results: [{ 0: { transcript: 'partial text' }, isFinal: false }],
    })
    expect(onResult).toHaveBeenCalledWith({ partial: 'partial text', isFinal: false })
  })

  it('silently ignores no-speech errors', async () => {
    const handles = installMock()
    const onError = vi.fn()
    const p = createVoiceProvider({ kind: 'webspeech' })
    await p.start({ onResult: vi.fn(), onError })
    handles[0].onerror?.({ error: 'no-speech' })
    expect(onError).not.toHaveBeenCalled()
  })

  it('forwards real errors with code and message', async () => {
    const handles = installMock()
    const onError = vi.fn()
    const p = createVoiceProvider({ kind: 'webspeech' })
    await p.start({ onResult: vi.fn(), onError })
    handles[0].onerror?.({ error: 'audio-capture' })
    expect(onError).toHaveBeenCalledTimes(1)
    expect(onError.mock.calls[0][0].code).toBe('audio-capture')
  })
})
