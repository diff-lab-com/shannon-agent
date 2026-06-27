import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createTtsSpeaker, isTtsSupported, listVoices, pickVoice } from '@/lib/voice/tts'

// jsdom doesn't ship SpeechSynthesisUtterance — install a minimal stub.
class StubUtterance {
  text: string
  lang = ''
  voice: SpeechSynthesisVoice | null = null
  rate = 1
  pitch = 1
  volume = 1
  onstart: ((e: Event) => void) | null = null
  onend: ((e: Event) => void) | null = null
  onerror: ((e: SpeechSynthesisErrorEvent) => void) | null = null
  constructor(text: string) { this.text = text }
}
;(globalThis as unknown as { SpeechSynthesisUtterance: typeof StubUtterance }).SpeechSynthesisUtterance = StubUtterance

type SynthMock = {
  speak: ReturnType<typeof vi.fn>
  cancel: ReturnType<typeof vi.fn>
  pause: ReturnType<typeof vi.fn>
  resume: ReturnType<typeof vi.fn>
  getVoices: ReturnType<typeof vi.fn>
}

function installSynth(voices: SpeechSynthesisVoice[] = []): { synth: SynthMock; utterances: SpeechSynthesisUtterance[] } {
  const utterances: SpeechSynthesisUtterance[] = []
  const synth: SynthMock = {
    speak: vi.fn((u: SpeechSynthesisUtterance) => {
      utterances.push(u)
      // Simulate async start.
      setTimeout(() => u.onstart?.(new Event('start')), 0)
    }),
    cancel: vi.fn(),
    pause: vi.fn(),
    resume: vi.fn(),
    getVoices: vi.fn(() => voices),
  }
  Object.defineProperty(window, 'speechSynthesis', { value: synth, configurable: true })
  return { synth, utterances }
}

describe('isTtsSupported', () => {
  afterEach(() => {
    delete (window as unknown as { speechSynthesis?: unknown }).speechSynthesis
  })

  it('returns false when speechSynthesis missing', () => {
    expect(isTtsSupported()).toBe(false)
  })

  it('returns true when speechSynthesis exists', () => {
    installSynth()
    expect(isTtsSupported()).toBe(true)
  })
})

describe('listVoices / pickVoice', () => {
  beforeEach(() => {
    installSynth([
      { lang: 'en-US', voiceURI: 'en-US-1', name: 'US English' } as SpeechSynthesisVoice,
      { lang: 'zh-CN', voiceURI: 'zh-CN-1', name: 'Chinese' } as SpeechSynthesisVoice,
    ])
  })

  afterEach(() => {
    delete (window as unknown as { speechSynthesis?: unknown }).speechSynthesis
  })

  it('listVoices returns underlying voice array', () => {
    expect(listVoices()).toHaveLength(2)
  })

  it('pickVoice prefers exact voiceURI match', () => {
    const v = pickVoice('en-US', 'zh-CN-1')
    expect(v?.voiceURI).toBe('zh-CN-1')
  })

  it('pickVoice falls back to first matching lang prefix', () => {
    const v = pickVoice('zh')
    expect(v?.lang).toBe('zh-CN')
  })

  it('pickVoice returns first voice when no match', () => {
    const v = pickVoice('fr-FR')
    expect(v).toBeDefined()
  })
})

describe('createTtsSpeaker', () => {
  afterEach(() => {
    delete (window as unknown as { speechSynthesis?: unknown }).speechSynthesis
  })

  it('isSupported mirrors runtime capability', () => {
    const s = createTtsSpeaker()
    expect(s.isSupported()).toBe(false)
    installSynth()
    const s2 = createTtsSpeaker()
    expect(s2.isSupported()).toBe(true)
  })

  it('speak is a no-op when unsupported', () => {
    const s = createTtsSpeaker()
    s.speak('hi')
    expect(s.state).toBe('idle')
  })

  it('speak routes through speechSynthesis and emits state changes', async () => {
    const { synth, utterances } = installSynth()
    const states: string[] = []
    const s = createTtsSpeaker({ onStateChange: (st) => states.push(st) })
    s.speak('hello world')
    expect(synth.speak).toHaveBeenCalledTimes(1)
    expect(utterances).toHaveLength(1)
    expect(utterances[0].text).toBe('hello world')
    await new Promise(r => setTimeout(r, 5))
    expect(states).toContain('speaking')
  })

  it('speak cancels previous utterance when called mid-flight', async () => {
    const { synth } = installSynth()
    const s = createTtsSpeaker()
    s.speak('first')
    s.speak('second')
    // First speak cancels anything pre-existing; second cancels the first.
    expect(synth.cancel).toHaveBeenCalledTimes(2)
    expect(synth.speak).toHaveBeenCalledTimes(2)
  })

  it('speak with empty string cancels and does not enqueue', () => {
    const { synth } = installSynth()
    const s = createTtsSpeaker()
    s.speak('')
    expect(synth.speak).not.toHaveBeenCalled()
    expect(synth.cancel).toHaveBeenCalled()
  })

  it('cancel resets state to idle', () => {
    installSynth()
    const s = createTtsSpeaker()
    s.speak('hi')
    s.cancel()
    expect(s.state).toBe('idle')
  })

  it('clamps rate/pitch/volume to allowed ranges', async () => {
    const { utterances } = installSynth()
    const s = createTtsSpeaker({ rate: 99, pitch: -5, volume: 5 })
    s.speak('extremes')
    await new Promise(r => setTimeout(r, 5))
    expect(utterances[0].rate).toBe(10)
    expect(utterances[0].pitch).toBe(0)
    expect(utterances[0].volume).toBe(1)
  })

  it('interrupted error events are silently treated as idle', async () => {
    const { utterances } = installSynth()
    const onError = vi.fn()
    const s = createTtsSpeaker({ onError })
    s.speak('hi')
    await new Promise(r => setTimeout(r, 5))
    const evt = { error: 'interrupted' } as unknown as SpeechSynthesisErrorEvent
    utterances[0].onerror?.(evt)
    expect(onError).not.toHaveBeenCalled()
    expect(s.state).toBe('idle')
  })

  it('non-interrupted errors surface via onError', async () => {
    const { utterances } = installSynth()
    const onError = vi.fn()
    const s = createTtsSpeaker({ onError })
    s.speak('hi')
    await new Promise(r => setTimeout(r, 5))
    const evt = { error: 'audio-busy' } as unknown as SpeechSynthesisErrorEvent
    utterances[0].onerror?.(evt)
    expect(onError).toHaveBeenCalledWith('audio-busy')
    expect(s.state).toBe('idle')
  })
})
