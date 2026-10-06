// Office Wave 3 C3 — companion Quick Capture page.
//
// Rendered inside the dedicated `companion` window (boots `/companion` via
// `open_companion_window`) and, as a fallback, at the same route in the
// main window. Deliberately chrome-less: a capture box, a Send action, a
// stay-on-top toggle.
//
// Trust contract (same as the Wave 2 composer bridge): Send never transmits
// a message — it emits `shannon:companion-prompt` to the main window, whose
// listener pushes the text into the chat composer as a DRAFT the user
// reviews and sends. All companion-side state is in-memory; nothing here
// persists.

import { useState } from 'react'
import { useT } from '@/i18n'
import { emitCompanionPrompt } from '@/lib/companionBridge'
import { setCompanionAlwaysOnTop } from '@/lib/tauri-api'

export default function CompanionPage() {
  const t = useT()
  const [text, setText] = useState('')
  const [sent, setSent] = useState(false)
  // Which operation failed — each has its own message. The raw error is
  // logged to the console (the window is too small for stack traces).
  const [error, setError] = useState<'send' | 'pin' | null>(null)
  // The companion window is created with always-on-top ON (Rust side), so
  // the checkbox starts checked; the Rust command is the single source of
  // truth once toggled.
  const [alwaysOnTop, setAlwaysOnTop] = useState(true)
  const [sending, setSending] = useState(false)

  const canSend = text.trim().length > 0 && !sending

  async function handleSend() {
    const trimmed = text.trim()
    if (!trimmed || sending) return
    setSending(true)
    setError(null)
    try {
      await emitCompanionPrompt(trimmed)
      setSent(true)
      setText('')
    } catch (e) {
      console.error('[companion] send failed', e)
      setError('send')
    } finally {
      setSending(false)
    }
  }

  async function handleAlwaysOnTopChange(next: boolean) {
    const previous = alwaysOnTop
    setAlwaysOnTop(next) // optimistic — the toggle must feel instant
    try {
      await setCompanionAlwaysOnTop(next)
    } catch (e) {
      console.error('[companion] always-on-top toggle failed', e)
      setAlwaysOnTop(previous)
      setError('pin')
    }
  }

  return (
    <div className="flex h-screen min-h-0 flex-col gap-3 p-3" data-testid="companion-page">
      <div className="flex items-center justify-between gap-2">
        <h1 className="text-body-md font-medium">{t('companion.title')}</h1>
        <label className="flex cursor-pointer items-center gap-1.5 text-label-sm text-on-surface-variant">
          <input
            type="checkbox"
            checked={alwaysOnTop}
            onChange={(e) => void handleAlwaysOnTopChange(e.target.checked)}
            aria-label={t('companion.alwaysOnTop')}
            data-testid="companion-always-on-top"
          />
          {t('companion.alwaysOnTop')}
        </label>
      </div>

      <textarea
        className="min-h-0 flex-1 resize-none rounded-xl border border-outline-variant bg-surface-container p-2 text-body-sm outline-none focus:border-primary"
        value={text}
        placeholder={t('companion.placeholder')}
        aria-label={t('companion.inputLabel')}
        onChange={(e) => {
          setText(e.target.value)
          setSent(false)
          setError(null)
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault()
            void handleSend()
          }
        }}
        data-testid="companion-input"
      />

      <div className="flex items-center justify-between gap-2" aria-live="polite">
        <span className="min-w-0 flex-1 truncate text-label-sm text-on-surface-variant">
          {/* Exactly one status at a time: success ack, then error, else empty. */}
          {error ? (
            <span className="text-error" data-testid="companion-error">
              {t(error === 'send' ? 'companion.errorSend' : 'companion.errorPin')}
            </span>
          ) : sent ? (
            <span data-testid="companion-sent">{t('companion.sent')}</span>
          ) : (
            ''
          )}
        </span>
        <button
          type="button"
          className="rounded-full bg-primary px-4 py-1.5 text-label-md font-medium text-on-primary disabled:opacity-50"
          onClick={() => void handleSend()}
          disabled={!canSend}
          data-testid="companion-send"
        >
          {t('companion.send')}
        </button>
      </div>
    </div>
  )
}
