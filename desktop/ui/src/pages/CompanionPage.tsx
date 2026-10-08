// Office Wave 3 C3 — companion Quick Capture page.
//
// Rendered inside the dedicated `companion` window (boots `/companion` via
// `open_companion_window`) and, as a fallback, at the same route in the
// main window. Deliberately chrome-less: a capture box, a Send action, a
// stay-on-top toggle, the in-memory capture history and the contract hint
// footer (design 13).
//
// Trust contract (same as the Wave 2 composer bridge): Send never transmits
// a message — it emits `shannon:companion-prompt` to the main window, whose
// listener pushes the text into the chat composer as a DRAFT the user
// reviews and sends. All companion-side state is in-memory; nothing here
// persists.
//
// Window contract (design 13:105「失焦自动收起 · Esc 关闭」): the Rust side
// hides the window on blur (`WindowEvent::Focused`); Esc here hides it
// through the `hide_companion_window` command — a Rust command, not the JS
// window API, so the companion capability stays event-only (same rationale
// as the always-on-top toggle).

import { useEffect, useRef, useState } from 'react'
import { FormattedRelativeTime } from 'react-intl'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useT } from '@/i18n'
import { emitCompanionPrompt } from '@/lib/companionBridge'
import { hideCompanionWindow, setCompanionAlwaysOnTop } from '@/lib/tauri-api'

/** Design 13:137-152 — at most five recent captures, newest first. */
const HISTORY_CAP = 5

interface CaptureEntry {
  id: number
  text: string
  /** Wall-clock ms the capture was sent (relative-time rendering only). */
  at: number
}

/**
 * Whether THIS webview is the dedicated `companion` window. Label-derived
 * (the JS window API works in every real webview); jsdom has no Tauri
 * internals, so failures read as "not the companion window" — which is the
 * safe default: Esc simply does nothing where hiding would be wrong anyway
 * (main-window fallback route, tests, plain browser).
 */
function isCompanionWebview(): boolean {
  try {
    return getCurrentWindow().label === 'companion'
  } catch {
    return false
  }
}

/** Props for <FormattedRelativeTime/> describing `atMs` relative to `nowMs`. */
function relativeTime(atMs: number, nowMs: number): { value: number; unit: 'second' | 'minute' | 'hour' | 'day' } {
  const seconds = Math.round((atMs - nowMs) / 1000) // negative = past
  if (seconds > -60) return { value: seconds, unit: 'second' }
  if (seconds > -3600) return { value: Math.round(seconds / 60), unit: 'minute' }
  if (seconds > -86400) return { value: Math.round(seconds / 3600), unit: 'hour' }
  return { value: Math.round(seconds / 86400), unit: 'day' }
}

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
  // In-memory capture history (design 13:137-152, audit ruling B5: keep it
  // in memory, never persist). Reopening the window starts from scratch —
  // the same ephemerality philosophy as the rest of this page.
  const [history, setHistory] = useState<CaptureEntry[]>([])
  const nextCaptureIdRef = useRef(0)
  // Ticks every 30s so the relative timestamps stay honest in a window the
  // user parks open.
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000)
    return () => window.clearInterval(timer)
  }, [])

  const canSend = text.trim().length > 0 && !sending

  // Esc collapses the window (design 13:105). Only wired in the dedicated
  // companion webview — on the main-window fallback route Esc must not hide
  // the whole app.
  useEffect(() => {
    if (!isCompanionWebview()) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      hideCompanionWindow().catch(err => console.error('[companion] esc-hide failed', err))
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  async function handleSend() {
    const trimmed = text.trim()
    if (!trimmed || sending) return
    setSending(true)
    setError(null)
    try {
      await emitCompanionPrompt(trimmed)
      setSent(true)
      setText('')
      setHistory(prev =>
        [{ id: nextCaptureIdRef.current++, text: trimmed, at: Date.now() }, ...prev].slice(0, HISTORY_CAP),
      )
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
    <div className="flex h-screen min-h-0 flex-col" data-testid="companion-page">
      <div className="flex items-center justify-between gap-2 border-b border-outline-variant/40 px-3 py-2">
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

      <div className="flex min-h-0 flex-1 flex-col gap-2 p-3">
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

      {/* Recent captures — memory only (never persisted; cleared when the
          window reopens). Text truncates with a title tooltip, matching the
          mockup's single-line rows. */}
      {history.length > 0 && (
        <div
          className="min-h-0 overflow-y-auto border-t border-outline-variant/40"
          aria-label={t('companion.history.label')}
          data-testid="companion-history"
        >
          {history.map(entry => (
            <div
              key={entry.id}
              className="flex items-center gap-2 px-3.5 py-2"
              data-testid="companion-history-item"
            >
              <span className="min-w-0 flex-1 truncate text-body-sm text-on-surface-variant" title={entry.text}>
                {entry.text}
              </span>
              <span className="shrink-0 font-mono text-label-xs text-on-surface-variant">
                <FormattedRelativeTime {...relativeTime(entry.at, now)} numeric="auto" />
              </span>
            </div>
          ))}
        </div>
      )}

      {/* Contract hint (design 13:155) — always visible so the draft-only
          promise stays one glance away. */}
      <div
        className="border-t border-outline-variant/40 px-3.5 py-1.5 font-mono text-label-xs text-on-surface-variant"
        data-testid="companion-footer"
      >
        {t('companion.footer.hint')}
      </div>
    </div>
  )
}
