// ChatStatusBar — Aurora redesign 2026-10 (02-chat.html 状态条), slimmed by
// the 02c 收敛裁决 (D5, 2026-10-14).
//
// Pins what is LEFT: the working-directory segment (the app's only WD-picker
// entry: same aria label, breadcrumb, disabled-without-session behavior)
// and the git-branch segment (hidden for non-git dirs, never stale).
//
// Pins what is GONE: the context / cache / spend / budget number segments —
// D4/D5 moved those into the composer's usage dialog; the status bar no
// longer shows numbers or money, even when a usage payload rides in.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'

import ChatStatusBar from '@/components/chat/ChatStatusBar'

// Branch segment IPC — defaults to "not a git repo" (null → hidden); the
// branch tests below override the resolved value per case.
const tauriApi = vi.hoisted(() => ({ currentGitBranch: vi.fn() }))
vi.mock('@/lib/tauri-api', () => ({ currentGitBranch: tauriApi.currentGitBranch }))

function renderBar(workingDir = '/home/alice/code/myproject', sessionId: string | null = 'session-1') {
  return render(
    <I18nProvider>
      <ChatStatusBar
        workingDir={workingDir}
        sessionId={sessionId}
        onChangeWorkingDir={() => {}}
      />
    </I18nProvider>,
  )
}

beforeEach(() => {
  tauriApi.currentGitBranch.mockResolvedValue(null)
})

describe('ChatStatusBar — working directory segment', () => {
  it('renders the breadcrumb and keeps the WD-picker entry point', () => {
    renderBar()
    const button = screen.getByRole('button', { name: 'Change working directory for this chat' })
    expect(button).toBeEnabled()
    expect(screen.getByText('…/code/myproject')).toBeInTheDocument()
  })

  it('shows "Not set" and disables the picker without a session', () => {
    renderBar('', null)
    expect(screen.getByRole('button', { name: 'Change working directory for this chat' })).toBeDisabled()
    expect(screen.getByText('Not set')).toBeInTheDocument()
  })
})

describe('ChatStatusBar — branch segment', () => {
  it('renders the working dir branch once the IPC resolves', async () => {
    tauriApi.currentGitBranch.mockResolvedValue('fix/billing-webhook')
    renderBar()
    const seg = await screen.findByTestId('chat-status-branch')
    expect(seg).toHaveTextContent('fix/billing-webhook')
    expect(tauriApi.currentGitBranch).toHaveBeenCalledWith('/home/alice/code/myproject')
  })

  it('hides the branch segment when the dir is not a git repo', async () => {
    renderBar()
    await waitFor(() => expect(tauriApi.currentGitBranch).toHaveBeenCalled())
    expect(screen.queryByTestId('chat-status-branch')).not.toBeInTheDocument()
  })
})

describe('ChatStatusBar — 02c D5: the number segments are gone', () => {
  it('never renders context / cache / cost / budget segments (they live in the usage dialog now)', () => {
    renderBar()
    expect(screen.queryByTestId('chat-status-context')).toBeNull()
    expect(screen.queryByTestId('chat-status-cache')).toBeNull()
    expect(screen.queryByTestId('chat-status-cost')).toBeNull()
    expect(screen.queryByTestId('chat-status-budget-left')).toBeNull()
  })
})
