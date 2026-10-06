import { useCallback, useEffect, useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Modal } from '@/components/ui/modal'
import { cn } from '@/lib/utils'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import { useTauriEventValidated } from '@/hooks/useTauriEventValidated'
import { useSessions } from '@/context/SessionContext'
import { EVENT_NAMES, type AskUserRequest } from '@/types'

type AskStatus = 'pending' | 'answered' | 'timedout'

interface CardState {
  req: AskUserRequest
  status: AskStatus
}

/** How long the settled (answered / timed-out) card lingers before
 *  auto-dismissing — long enough to read the outcome, short enough to get
 *  out of the way of the next question. */
const SETTLED_AUTO_DISMISS_MS = 6000

/**
 * Settings R3 T8 — the desktop ask_user question dialog (C3 + R8).
 *
 * The engine's `ask_user_question` tool used to read stdin, which is
 * unusable under a GUI (EOF → error, or a hung read). The desktop backend
 * swaps in `DesktopQuestionHandler`, which emits `ask-user-request` and
 * waits; this card renders the question in the SAME surface slot as the
 * approval dialog (Header, right next to the permission modal) and answers
 * through `api.respondAskUser(requestId, answers)`.
 *
 * States: pending (options + free text) → answered (submit) — or, when
 * 提问自动继续 is on and the user doesn't answer in time, the backend fires
 * `ask-user-resolved` (timed_out) and the card shows the auto-continued
 * state. `timeout_ms` on the request drives a pure-display mm:ss countdown;
 * the backend timeout is the authority.
 *
 * F2 session scoping: the request carries the live run's `session_id` when
 * exactly one run is active — a scoped card renders ONLY in windows viewing
 * that session (`windowSessionId` pin, else the active session); an absent
 * `session_id` (ambiguous run) keeps the every-window fallback. Both
 * resolution paths broadcast `ask-user-resolved` — an answered-elsewhere
 * broadcast clears the still-pending card here, a timed-out broadcast shows
 * the auto-continued cue.
 */
export default function AskUserCard() {
  const intl = useIntl()
  const t = useCallback(
    (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values),
    [intl],
  )
  const { currentSessionId, windowSessionId } = useSessions()

  const [card, setCard] = useState<CardState | null>(null)
  const [selected, setSelected] = useState<string[]>([])
  const [custom, setCustom] = useState('')
  const [remainingSecs, setRemainingSecs] = useState<number | null>(null)

  useTauriEventValidated<AskUserRequest>(EVENT_NAMES.ASK_USER_REQUEST, (e) => {
    const req = e.payload
    if (!req || typeof req.request_id !== 'string') return
    setCard({ req, status: 'pending' })
    setSelected([])
    setCustom('')
    setRemainingSecs(req.timeout_ms != null ? Math.max(0, Math.round(req.timeout_ms / 1000)) : null)
  })

  useTauriEventValidated<{ request_id: string; timed_out: boolean }>(
    EVENT_NAMES.ASK_USER_RESOLVED,
    (e) => {
      setCard((cur) => {
        if (!cur || cur.req.request_id !== e.payload.request_id) return cur
        // Timed out elsewhere → the auto-continued cue (backend already
        // answered with the best-judgment text).
        if (e.payload.timed_out) return { ...cur, status: 'timedout' }
        // Answered — here or in another window. A card already in the
        // answered state (THIS window submitted it) keeps its brief settled
        // linger; a still-pending card was answered elsewhere → clear it.
        return cur.status === 'answered' ? cur : null
      })
    },
  )

  // Pure-display countdown (1 Hz) while a bounded question is pending. The
  // backend drives the actual timeout — the resolved event settles the card
  // even if this display still shows a second or two.
  useEffect(() => {
    if (card?.status !== 'pending' || remainingSecs == null || remainingSecs <= 0) return
    const iv = setInterval(() => {
      setRemainingSecs((s) => (s != null && s > 0 ? s - 1 : 0))
    }, 1000)
    return () => clearInterval(iv)
  }, [card?.status, remainingSecs])

  // Settled cards linger briefly, then auto-dismiss.
  useEffect(() => {
    if (card?.status !== 'answered' && card?.status !== 'timedout') return
    const to = setTimeout(() => setCard(null), SETTLED_AUTO_DISMISS_MS)
    return () => clearTimeout(to)
  }, [card?.status, card?.req.request_id])

  const submit = useCallback(
    async (raw: string[]) => {
      if (!card || card.status !== 'pending' || raw.length === 0) return
      const requestId = card.req.request_id
      setCard({ req: card.req, status: 'answered' })
      try {
        await api.respondAskUser(requestId, raw)
      } catch (e) {
        toastError(t('chat.askUser.sendFailed'), e)
        // Put the form back so the user can retry.
        setCard((cur) =>
          cur && cur.req.request_id === requestId ? { ...cur, status: 'pending' } : cur,
        )
      }
    },
    [card, t],
  )

  // F2 render-level session scoping: show the card when the request is
  // unscoped (ambiguous run → every window, pre-F2 behavior) or when it
  // belongs to the session THIS window is viewing (a pinned session window
  // wins, else the active chat session). Evaluated at render time, so
  // switching sessions hides/reveals a pending card without an event replay.
  const visibleSessionId = windowSessionId ?? currentSessionId
  if (!card) return null
  const { req, status } = card
  if (req.session_id != null && req.session_id !== visibleSessionId) return null
  const trimmed = custom.trim()
  const canSend = trimmed.length > 0 || selected.length > 0

  const mmss = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`

  return (
    <Modal
      open
      onClose={() => setCard(null)}
      size="md"
      role="dialog"
      showCloseButton={false}
      ariaLabelledBy="ask-user-title"
      testId="ask-user-dialog"
    >
      <div className="p-xl" data-testid="ask-user-card">
        <div className="flex items-center gap-md mb-lg">
          <div className="h-10 w-10 rounded-full bg-secondary-container flex items-center justify-center shrink-0">
            <span className="material-symbols-outlined text-on-secondary-container" aria-hidden="true">contact_support</span>
          </div>
          <div className="flex-1 min-w-0">
            <h3 id="ask-user-title" className="font-headline-sm text-on-surface font-bold">{t('chat.askUser.title')}</h3>
            {req.header ? (
              <p className="text-body-sm text-on-surface-variant">{req.header}</p>
            ) : null}
          </div>
          {status === 'pending' && remainingSecs != null && (
            <span
              data-testid="ask-user-countdown"
              aria-label={t('chat.askUser.countdownAria', { time: mmss(remainingSecs) })}
              className="px-sm py-xs rounded-full bg-tertiary-container text-on-tertiary-container font-label-sm font-bold tabular-nums whitespace-nowrap"
            >
              {mmss(remainingSecs)}
            </span>
          )}
        </div>

        <p className="font-body-md text-on-surface mb-md break-words" data-testid="ask-user-question">
          {req.question}
        </p>

        {status === 'pending' ? (
          <>
            {req.options.length > 0 && (
              <div
                className="flex flex-wrap gap-sm mb-md"
                role="group"
                aria-label={t('chat.askUser.optionsLabel')}
                data-testid="ask-user-options"
              >
                {req.options.map((o) => {
                  const active = selected.includes(o.label)
                  return (
                    <button
                      key={o.label}
                      type="button"
                      data-testid="ask-user-option"
                      aria-pressed={active}
                      title={o.description || undefined}
                      onClick={() =>
                        req.multi_select
                          ? setSelected((cur) =>
                              cur.includes(o.label)
                                ? cur.filter((l) => l !== o.label)
                                : [...cur, o.label],
                            )
                          : void submit([o.label])
                      }
                      className={cn(
                        'px-md py-sm rounded-xl border font-label-md transition-all',
                        active
                          ? 'bg-primary text-on-primary border-primary'
                          : 'bg-surface-container text-on-surface border-outline-variant/40 hover:bg-surface-container-high',
                      )}
                    >
                      {o.label}
                    </button>
                  )
                })}
              </div>
            )}
            <div className="flex gap-md">
              <input
                type="text"
                value={custom}
                onChange={(e) => setCustom(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && trimmed) void submit([trimmed])
                }}
                placeholder={t('chat.askUser.customPlaceholder')}
                aria-label={t('chat.askUser.customPlaceholder')}
                data-testid="ask-user-input"
                className="flex-1 min-w-0 px-md py-sm bg-surface-container rounded-xl text-body-md text-on-surface outline-none focus:ring-2 focus:ring-primary border border-outline-variant/40 placeholder:text-on-surface-variant"
              />
              <Button
                type="button"
                data-testid="ask-user-send"
                disabled={!canSend}
                onClick={() => void submit(trimmed ? [trimmed] : selected)}
                className="shrink-0 px-lg py-sm bg-primary text-on-primary rounded-xl hover:shadow-md hover:shadow-primary/30 transition-all font-label-md disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {t('chat.askUser.send')}
              </Button>
            </div>
          </>
        ) : (
          <div
            role="status"
            data-testid="ask-user-status"
            className={cn(
              'flex items-center gap-sm px-md py-sm rounded-xl text-label-md',
              status === 'timedout'
                ? 'bg-warning-container text-on-warning-container'
                : 'bg-primary-container text-on-primary-container',
            )}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">
              {status === 'timedout' ? 'timer_off' : 'check_circle'}
            </span>
            <span>{status === 'timedout' ? t('chat.askUser.timedOut') : t('chat.askUser.answered')}</span>
          </div>
        )}
      </div>
    </Modal>
  )
}
