// SessionsPanel — minimal session switcher (P2-5b spike, see
// `docs/plans/chat-upgrade.md` §3.2 / the per-session event queue
// tasks #54–#55). Single-purpose component: list every session the
// Rust backend knows about, mark the active one, and switch the
// `currentSessionId` in `AppContext` when clicked.
//
// This is deliberately small. The full thread-switcher UX
// (per-session event replay, focused-stream rendering, unread
// indicators, fork/branch/rename) lives in the next iteration; this
// spike just proves the wiring end-to-end so the next iteration has
// a handhold.
//
// State sources:
//   - `useSessions()` — `sessions: SessionInfo[]`,
//     `currentSessionId: string | null`,
//     `switchSession(id)`, `createSession()`, `refreshSessions()`.
//   - `useCatalog()` — `error: string | null`,
//     `setError` (via rethrow) for bad-switch reporting.
//
// Behaviour:
//   - On mount: `refreshSessions()` to ensure the list reflects the
//     latest backend state (a user may have switched via keyboard
//     shortcut between renders).
//   - On click: call `switchSession(id)`. On rejection, surface the
//     error in the catalog slice's `setError`. Note: `setError` is
//     exposed via `useApp()` but not via the slice; we use the
//     legacy `useApp()` composer's `setError`-equivalent by reading
//     `useCatalog().error` and letting the parent pick up state.
//   - "New thread" button: call `createSession()`. The existing
//     `createSession` already sets `currentSessionId` in AppContext
//     and clears messages — no extra wiring needed.

import { useCallback, useEffect, useMemo } from 'react'
import { useIntl } from 'react-intl'
// icon imports dropped — using material-symbols-outlined spans per T0.2
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '@/lib/utils'
import { useSessions } from '@/context/SessionContext'
import { useApp } from '@/context/AppContext'

export interface SessionsPanelProps {
  /** Optional className for layout integration. */
  className?: string
  /** Locale message id for the panel title. Defaults to a generic label. */
  titleId?: string
}

/**
 * Render a vertical list of every session with the active one highlighted.
 * Always-visible, even when there's only one session — the user can still
 * create a new thread without leaving the panel.
 */
export function SessionsPanel({ className, titleId }: SessionsPanelProps) {
  const intl = useIntl()
  const title = intl.formatMessage({
    id: titleId ?? 'sessionsPanel.title',
    defaultMessage: 'Threads',
  })

  const sessions = useSessions().sessions
  const currentSessionId = useSessions().currentSessionId
  const switchSession = useSessions().switchSession
  const createSession = useSessions().createSession
  const refreshSessions = useSessions().refreshSessions
  const app = useApp()

  // Ensure the list is fresh on mount (cheap: a single `listSessions` invoke).
  useEffect(() => {
    // Use `setError` indirectly via the catalog slice's `error` field by
    // dispatching through the parent AppContext composer's `error` setter
    // — `useApp()` doesn't expose one of those either, so we surface
    // refresh failures in the parent log instead and just `void` reject.
    refreshSessions().catch((e) => {
      console.warn('SessionsPanel: refreshSessions failed:', e)
    })
    // Intentionally `[]` — refreshSessions is stable across renders per
    // the AppContext memo. We only need to fire once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Stable callbacks so the list items don't re-render on every parent update.
  const handleCreate = useCallback(async () => {
    try {
      await createSession()
    } catch (e) {
      console.warn('SessionsPanel: createSession failed:', e)
    }
  }, [createSession])

  const handleSwitch = useCallback(
    async (id: string) => {
      if (id === currentSessionId) return
      try {
        await switchSession(id)
      } catch (e) {
        console.warn('SessionsPanel: switchSession failed:', e)
      }
    },
    [switchSession, currentSessionId],
  )

  // `app` is referenced solely so the component re-renders if any
  // cross-slice state (catalog loading/error/etc.) changes — keeps the
  // panel in sync with parent state without forcing a wider consumer
  // network.
  void app

  // Sort sessions so the most recently created one is at the top of
  // the list (matches the sidebar's natural reading order).
  const ordered = useMemo(() => {
    return [...sessions].sort((a, b) => b.created_at - a.created_at)
  }, [sessions])

  return (
    <aside
      className={cn(
        'flex flex-col w-56 border-r border-on-surface-variant/20 bg-surface-container-low',
        className,
      )}
      aria-label={title}
    >
      <div className="flex items-center justify-between px-3 py-2 border-b border-on-surface-variant/20">
        <h2 className="font-label-md text-sm font-semibold text-on-surface">{title}</h2>
        <Button
          variant="ghost"
          size="icon-xs"
          onClick={handleCreate}
          aria-label={intl.formatMessage({
            id: 'sessionsPanel.createThread',
            defaultMessage: 'New thread',
          })}
          title={intl.formatMessage({
            id: 'sessionsPanel.createThread.tooltip',
            defaultMessage: 'Start a new thread',
          })}
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">add</span>
        </Button>
      </div>
      <ScrollArea className="flex-1 min-h-0">
        {ordered.length === 0 ? (
          <EmptyState />
        ) : (
          <ul className="flex flex-col gap-1 p-2">
            {ordered.map((s) => (
              <li key={s.id}>
                <SessionRow
                  id={s.id}
                  title={s.title ?? shortId(s.id)}
                  messageCount={s.message_count ?? 0}
                  active={s.id === currentSessionId}
                  onSelect={handleSwitch}
                />
              </li>
            ))}
          </ul>
        )}
      </ScrollArea>
    </aside>
  )
}

interface SessionRowProps {
  id: string
  title: string
  messageCount: number
  active: boolean
  onSelect: (id: string) => void
}

function SessionRow({ id, title, messageCount, active, onSelect }: SessionRowProps) {
  const intl = useIntl()
  return (
    <Button
      variant="ghost"
      onClick={() => onSelect(id)}
      aria-current={active ? 'page' : undefined}
      className={cn(
        'h-auto justify-start items-start whitespace-normal text-left rounded-md px-2 py-2',
        active
          ? 'bg-primary/15 text-primary font-bold hover:bg-primary/15'
          : 'hover:bg-on-surface-variant/10 text-on-surface',
      )}
    >
      <span
        className={cn(
          'material-symbols-outlined mt-0.5 icon-sm shrink-0',
          active ? 'text-primary' : 'text-on-surface-variant',
        )}
        aria-hidden="true"
      >
        chat_bubble
      </span>
      <span className="flex-1 min-w-0">
        <span className="block truncate text-sm" title={title}>
          {title}
        </span>
        <span className="block text-xs text-on-surface-variant">
          {intl.formatMessage(
            { id: 'sessionsPanel.messageCount', defaultMessage: '{count, plural, =0 {No messages} one {# message} other {# messages}}' },
            { count: messageCount },
          )}
        </span>
      </span>
    </Button>
  )
}

function EmptyState() {
  const intl = useIntl()
  return (
    <div className="flex flex-col items-center justify-center px-4 py-8 text-center">
      <span className="material-symbols-outlined icon-md text-on-surface-variant/50 mb-2" aria-hidden="true">
        chat_bubble
      </span>
      <p className="text-sm text-on-surface-variant">
        {intl.formatMessage({
          id: 'sessionsPanel.empty',
          defaultMessage: 'No threads yet — click + to start one.',
        })}
      </p>
    </div>
  )
}

/** Fallback display for sessions whose title is missing: first 8 hex chars of the UUID. */
function shortId(id: string): string {
  if (id.length >= 8) return id.slice(0, 8)
  return id
}
