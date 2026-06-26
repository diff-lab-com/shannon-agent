import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { useVoice } from '@/hooks/useVoice'
import { MicButton } from '@/components/voice/MicButton'
import { VoiceOrb } from '@/components/voice/VoiceOrb'

function renderWithI18n(ui: React.ReactNode) {
  return render(<I18nProvider>{ui}</I18nProvider>)
}

describe('useVoice hook (stubbed backend)', () => {
  function VoiceProbe() {
    const v = useVoice({ simulateLatencyMs: 50 })
    return (
      <div>
        <div data-testid="state">{v.state}</div>
        <div data-testid="partial">{v.partialTranscript || 'empty'}</div>
        <button onClick={() => void v.startRecording()}>start</button>
        <button onClick={() => void v.stopRecording()}>stop</button>
        <button onClick={() => void v.speak('hi')}>speak</button>
        <button onClick={() => v.stopSpeaking()}>stopSpeak</button>
        <button onClick={() => v.reset()}>reset</button>
      </div>
    )
  }

  beforeEach(() => vi.clearAllMocks())

  it('starts idle with empty partial', () => {
    renderWithI18n(<VoiceProbe />)
    expect(screen.getByTestId('state')).toHaveTextContent('idle')
    expect(screen.getByTestId('partial')).toHaveTextContent('empty')
  })

  it('startRecording transitions to recording state', async () => {
    renderWithI18n(<VoiceProbe />)
    fireEvent.click(screen.getByText('start'))
    expect(screen.getByTestId('state')).toHaveTextContent('recording')
    expect(screen.getByTestId('partial')).toHaveTextContent('Listening')
  })

  it('stopRecording transitions through transcribing to idle and emits final transcript', async () => {
    const onTranscript = vi.fn()
    function Probe() {
      const v = useVoice({ simulateLatencyMs: 20, onTranscript })
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
