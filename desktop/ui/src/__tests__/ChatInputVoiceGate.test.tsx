// B4 P2-5: the composer's mic button is gated on `useVoice().supported` —
// without an STT provider there must be no mic to click (a control that
// only opens a doomed recording is worse than none).
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import type * as ReactRouterDom from 'react-router-dom'
import ChatInput from '@/components/chat/ChatInput'
import type { UseVoiceResult } from '@/hooks/useVoice'

const { fakeVoice } = vi.hoisted(() => ({
  fakeVoice: { current: null as UseVoiceResult | null },
}))

vi.mock('@/hooks/useVoice', () => ({
  useVoice: () => fakeVoice.current!,
}))

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: null,
    status: null,
    models: [],
    refreshConfig: async () => {},
    refreshStatus: async () => {},
  }),
}))

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return {
    ...actual,
    useNavigate: () => () => {},
    useOutletContext: () => ({ search: '' }),
  }
})

function baseVoice(supported: boolean): UseVoiceResult {
  return {
    state: 'idle',
    partialTranscript: '',
    error: null,
    supported,
    startRecording: async () => {},
    stopRecording: async () => {},
    reset: () => {},
  }
}

function renderInput() {
  return render(
    <ChatInput
      value=""
      onChange={() => {}}
      onSend={() => {}}
      onExecuteSlash={() => {}}
      attachedFiles={[]}
      onAttach={() => {}}
      onDetachAll={() => {}}
      isQuerying={false}
      onCancelQuery={() => {}}
      onOpenQuickFix={() => {}}
      onOpenEditor={() => {}}
    />,
  )
}

describe('ChatInput — mic gating on voice support (B4 P2-5)', () => {
  it('hides the mic button when the voice provider is unsupported', () => {
    fakeVoice.current = baseVoice(false)
    renderInput()
    expect(screen.queryByLabelText('Start voice recording')).not.toBeInTheDocument()
  })

  it('renders the mic button when the voice provider is supported', () => {
    fakeVoice.current = baseVoice(true)
    renderInput()
    expect(screen.getByLabelText('Start voice recording')).toBeInTheDocument()
  })
})
