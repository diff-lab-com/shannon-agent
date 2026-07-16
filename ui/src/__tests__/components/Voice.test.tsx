import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { useVoice } from '@/hooks/useVoice'
import { MicButton } from '@/components/voice/MicButton'
import { VoiceOrb } from '@/components/voice/VoiceOrb'
import * as api from '@/lib/tauri-api'

function renderWithI18n(ui: React.ReactNode) {
  return render(<I18nProvider>{ui}</I18nProvider>)
}

interface FakeRecorder {
  ondataavailable: ((e: { data: Blob }) => void) | null
  onstop: (() => void) | null
  start(): void
  stop(): void
}

/** Install a fake MediaRecorder + getUserMedia for the cloud-STT hook tests. */
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
  Object.defineProperty(navigator, 'mediaDevices', {
    value: { getUserMedia: vi.fn().mockResolvedValue({ getTracks: () => [{ stop: vi.fn() }] }) },
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

describe('useVoice hook (stub fallback without MediaRecorder)', () => {
  beforeEach(() => {
    // jsdom has no MediaRecorder by default → factory falls back to the stub.
    delete (globalThis as unknown as { MediaRecorder?: typeof MediaRecorder }).MediaRecorder
    vi.clearAllMocks()
  })

  function VoiceProbe() {
    const v = useVoice()
    return (
      <div>
        <div data-testid="state">{v.state}</div>
        <div data-testid="partial">{v.partialTranscript || 'empty'}</div>
        <div data-testid="supported">{v.supported ? 'yes' : 'no'}</div>
        <button onClick={() => void v.startRecording()}>start</button>
        <button onClick={() => void v.stopRecording()}>stop</button>
        <button onClick={() => void v.speak('hi')}>speak</button>
        <button onClick={() => v.stopSpeaking()}>stopSpeak</button>
        <button onClick={() => v.reset()}>reset</button>
      </div>
    )
  }

  it('starts idle with empty partial and reports supported (stub fallback)', () => {
    renderWithI18n(<VoiceProbe />)
    expect(screen.getByTestId('state')).toHaveTextContent('idle')
    expect(screen.getByTestId('partial')).toHaveTextContent('empty')
    expect(screen.getByTestId('supported')).toHaveTextContent('yes')
  })

  it('startRecording transitions to recording state', async () => {
    renderWithI18n(<VoiceProbe />)
    fireEvent.click(screen.getByText('start'))
    expect(screen.getByTestId('state')).toHaveTextContent('recording')
    expect(screen.getByTestId('partial')).toHaveTextContent('Listening')
  })

  it('stopRecording returns to idle and emits the final transcript', async () => {
    const onTranscript = vi.fn()
    function Probe() {
      const v = useVoice({ onTranscript })
      return (
        <div>
          <div data-testid="state">{v.state}</div>
          <button onClick={() => { void v.startRecording().then(() => void v.stopRecording()) }}>go</button>
        </div>
      )
    }
    renderWithI18n(<Probe />)
    fireEvent.click(screen.getByText('go'))
    await waitFor(() => expect(screen.getByTestId('state')).toHaveTextContent('idle'))
    expect(onTranscript).toHaveBeenCalledWith('This is a stub transcript. Real STT backend not configured.')
  })

  it('speak sets state to speaking', async () => {
    renderWithI18n(<VoiceProbe />)
    fireEvent.click(screen.getByText('speak'))
    expect(screen.getByTestId('state')).toHaveTextContent('speaking')
  })

  it('stopSpeaking returns to idle', async () => {
    renderWithI18n(<VoiceProbe />)
    fireEvent.click(screen.getByText('speak'))
    fireEvent.click(screen.getByText('stopSpeak'))
    expect(screen.getByTestId('state')).toHaveTextContent('idle')
  })

  it('reset clears state', async () => {
    renderWithI18n(<VoiceProbe />)
    fireEvent.click(screen.getByText('start'))
    fireEvent.click(screen.getByText('reset'))
    expect(screen.getByTestId('state')).toHaveTextContent('idle')
    expect(screen.getByTestId('partial')).toHaveTextContent('empty')
  })
})

describe('useVoice hook — cloud STT (remote provider)', () => {
  let teardown: () => void
  let instances: FakeRecorder[]

  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.transcribeAudio).mockResolvedValue({ text: 'cloud says hi' })
    ;({ teardown, instances } = installMediaRecorder())
  })
  afterEach(() => {
    teardown()
  })

  it('transcribes via the backend when MediaRecorder is available', async () => {
    const onTranscript = vi.fn()
    function Probe() {
      const v = useVoice({ onTranscript })
      return (
        <div>
          <div data-testid="state">{v.state}</div>
          <button onClick={() => void v.startRecording()}>start</button>
          <button onClick={() => void v.stopRecording()}>stop</button>
        </div>
      )
    }
    renderWithI18n(<Probe />)
    fireEvent.click(screen.getByText('start'))
    await waitFor(() => expect(instances.length).toBeGreaterThan(0))
    instances[0].ondataavailable!({ data: new Blob(['audio']) })
    fireEvent.click(screen.getByText('stop'))
    await waitFor(() => expect(onTranscript).toHaveBeenCalledWith('cloud says hi'))
  })
})

describe('MicButton', () => {
  it('renders mic icon when idle', () => {
    renderWithI18n(<MicButton state="idle" onStart={() => {}} onStop={() => {}} />)
    expect(screen.getByRole('button', { name: /Start voice recording/ })).toBeInTheDocument()
    expect(screen.getByRole('button')).toHaveAttribute('aria-pressed', 'false')
  })

  it('renders stop icon when recording and aria-pressed=true', () => {
    renderWithI18n(<MicButton state="recording" onStart={() => {}} onStop={() => {}} />)
    const btn = screen.getByRole('button', { name: /Stop recording/ })
    expect(btn).toHaveAttribute('aria-pressed', 'true')
  })

  it('calls onStart when clicked from idle', () => {
    const onStart = vi.fn()
    renderWithI18n(<MicButton state="idle" onStart={onStart} onStop={() => {}} />)
    fireEvent.click(screen.getByRole('button'))
    expect(onStart).toHaveBeenCalledTimes(1)
  })

  it('calls onStop when clicked while recording', () => {
    const onStop = vi.fn()
    renderWithI18n(<MicButton state="recording" onStart={() => {}} onStop={onStop} />)
    fireEvent.click(screen.getByRole('button'))
    expect(onStop).toHaveBeenCalledTimes(1)
  })

  it('is disabled when transcribing', () => {
    renderWithI18n(<MicButton state="transcribing" onStart={() => {}} onStop={() => {}} />)
    expect(screen.getByRole('button')).toBeDisabled()
  })

  it('is disabled when prop disabled=true', () => {
    renderWithI18n(<MicButton state="idle" disabled onStart={() => {}} onStop={() => {}} />)
    expect(screen.getByRole('button')).toBeDisabled()
  })
})

describe('VoiceOrb', () => {
  it('renders with given size', () => {
    const { container } = renderWithI18n(<VoiceOrb state="idle" size={80} />)
    const orb = container.querySelector('[role="presentation"]')
    expect(orb).toBeTruthy()
    expect(orb?.getAttribute('style') || '').toContain('80px')
  })

  it('uses default size 64', () => {
    const { container } = renderWithI18n(<VoiceOrb state="idle" />)
    const orb = container.querySelector('[role="presentation"]')
    expect(orb?.getAttribute('style') || '').toContain('64px')
  })

  it('applies error styling for recording state', () => {
    const { container } = renderWithI18n(<VoiceOrb state="recording" />)
    const orb = container.querySelector('[role="presentation"]')
    expect(orb?.className).toContain('bg-error')
  })

  it('applies primary styling for speaking state', () => {
    const { container } = renderWithI18n(<VoiceOrb state="speaking" />)
    const orb = container.querySelector('[role="presentation"]')
    expect(orb?.className).toContain('bg-primary')
  })
})
