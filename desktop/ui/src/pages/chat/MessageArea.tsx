import { useT } from '@/i18n'
import { Banner } from '@/components/ui/banner'
import type { RefObject } from 'react'
import { useEffect, useMemo, useRef, useState, useCallback } from 'react'
import type { Virtualizer } from '@tanstack/react-virtual'
import { Button } from '@/components/ui/button'
import WelcomeState from '@/components/WelcomeState'
import { MessageBubble, type RegeneratePayload } from '@/components/chat/MessageBubble'
import StreamingResponse from '@/components/chat/StreamingResponse'
import { useChat } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useSessions } from '@/context/SessionContext'
import { useComposer } from './ComposerContext'
import * as api from '@/lib/tauri-api'
// Virtualization only kicks in past the threshold. Below it, the overhead
// of measuring/positioning outweighs the win from fewer DOM nodes — and
// jsdom can't provide real dimensions, so tests would render zero items.
// (Exported: Chat's search bar needs the same threshold to pick the jump
// mechanism for a match.)
export const VIRTUALIZE_THRESHOLD = 30

/** djb2 hex — short stable salt for React keys of messages that carry no id. */
function hashContent(s: string): string {
  let h = 5381
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0
  return (h >>> 0).toString(36)
}

/**
 * P2-13 (§4-15): virtualizer keys must be stable identity, not index-bound.
 * ChatMessage carries no id, so the most stable available fingerprint is
 * role + timestamp + content hash, deduplicated in list order for the rare
 * identical twins.
 */
function useStableMessageKeys(messages: { role: string; content: string; timestamp: number }[]): string[] {
  return useMemo(() => {
    const seen = new Map<string, number>()
    return messages.map((m) => {
      const base = `${m.role}-${m.timestamp}-${hashContent(m.content)}`
      const n = seen.get(base) ?? 0
      seen.set(base, n + 1)
      return n === 0 ? base : `${base}#${n}`
    })
  }, [messages])
}

/**
 * P2-17 (§4-17): the single aria-live region for run state transitions.
 * The streaming log itself used to be a polite live region (screen-reader
 * token spam) — now only the transitions announce: "generating" when a run
 * starts, "reply complete" when it ends. Exported for direct testability.
 */
export function StreamStatusRegion({ active }: { active: boolean }) {
  const t = useT()
  const [message, setMessage] = useState('')
  const wasActiveRef = useRef(false)
  useEffect(() => {
    if (active) {
      wasActiveRef.current = true
      setMessage(t('chat.stream.status.active'))
    } else if (wasActiveRef.current) {
      wasActiveRef.current = false
      setMessage(t('chat.stream.status.done'))
    }
    // Transitions strictly alternate, so consecutive writes always change
    // the text — a polite live region announces each one; no clear/rewrite
    // dance (and no timer) needed.
  }, [active, t])
  return (
    <div role="status" aria-live="polite" className="sr-only" data-testid="stream-status-region">
      {message}
    </div>
  )
}

/**
 * P1-⑤ telemetry: tool_use_id → duration (ms) from the session's L0 trace
 * timeline — one IPC per session, giving every historical tool card its
 * authoritative duration (live cards measure client-side instead).
 * Best-effort: failures just leave the cards without a duration label.
 */
function useToolDurationLookup(sessionId: string | null): Map<string, number> {
  const [lookup, setLookup] = useState<Map<string, number>>(() => new Map())
  useEffect(() => {
    if (!sessionId) {
      setLookup(new Map())
      return
    }
    let cancelled = false
    api.getTraceTimeline(sessionId)
      .then(tl => {
        if (cancelled) return
        const map = new Map<string, number>()
        for (const turn of tl.turns) {
          for (const tool of turn.tools) {
            if (tool.duration_ms != null) map.set(tool.tool_use_id, tool.duration_ms)
          }
        }
        setLookup(map)
      })
      .catch(() => { /* durations are opportunistic */ })
    return () => { cancelled = true }
  }, [sessionId])
  return lookup
}

interface MessageAreaProps {
  scrollParentRef: RefObject<HTMLDivElement | null>
  messagesEndRef: RefObject<HTMLDivElement | null>
  virtualizer: Virtualizer<HTMLDivElement, Element>
  setDiffPath: (p: string | null) => void
  setDiffPaths: (p: string[] | null) => void
  /** B1 §4-12: the message a search jump landed on (transient ring). */
  searchFlashIndex?: number | null
  /** B1 §4-8: begin a composer-based edit of the user message at `index`. */
  onEditMessage?: (index: number) => void
}

/** B1 §4-7: everything the LAST assistant message needs for a true
 *  regenerate — rewind to the checkpoint before its preceding user turn and
 *  re-send that turn's text + attachment paths. Null when there is no
 *  preceding user turn or no checkpoint covers it (fresh/demo sessions):
 *  then the button simply does not render, like the rewind affordance.
 *  (Shape: RegeneratePayload, declared in MessageBubble.) */
function regenerateInfoFor(messages: { role: string; content: string; file_attachments?: { path: string }[] }[], lastAssistantIndex: number, checkpointTurns: number[]): RegeneratePayload | null {
  let userIdx = -1
  for (let i = lastAssistantIndex - 1; i >= 0; i--) {
    if (messages[i]?.role === 'user') {
      userIdx = i
      break
    }
  }
  if (userIdx < 0) return null
  const { turnIndex, rewindable } = rewindInfoFor(messages, userIdx, checkpointTurns)
  if (!rewindable) return null
  const userMessage = messages[userIdx]
  return {
    turnIndex,
    content: userMessage.content,
    attachmentPaths: (userMessage.file_attachments ?? []).map(a => a.path),
  }
}

/** True when the user is scrolled away from the very bottom by at least
 *  the threshold below. The chat page surfaces a "scroll to latest" FAB
 *  whenever this is the case — and also when the active run starts so the
 *  user can return to live output after reading an older turn. */
const SCROLL_FROM_BOTTOM_THRESHOLD_PX = 200

// Virtualized + non-virtualized message list. Below the threshold (30
// messages) we render everything in a flat log so jsdom tests still see the
// bubbles — virtualization's measureElement needs a real DOM with height.
// Per-message rewind affordance: a user message at chat index `msgIndex`
// owns conversation turn `count(user messages before it)`. It is rewindable
// when a recorded checkpoint sits at or after that turn — no checkpoints
// (e.g. fresh session, demo mode) means nothing to undo and no button.
function rewindInfoFor(messages: { role: string }[], msgIndex: number, checkpointTurns: number[]): { turnIndex: number; rewindable: boolean } {
  let turnIndex = 0
  for (let i = 0; i < msgIndex; i++) {
    if (messages[i]?.role === 'user') turnIndex++
  }
  const rewindable = checkpointTurns.some(t => t >= turnIndex)
  return { turnIndex, rewindable }
}

export default function MessageArea({
  scrollParentRef,
  messagesEndRef,
  virtualizer,
  setDiffPath,
  setDiffPaths,
  searchFlashIndex,
  onEditMessage,
}: MessageAreaProps) {
  const { messages, streamingText, thinkingText, activeToolCalls, checkpoints, rewindSession, isQuerying } = useChat()
  const { currentSessionId, sessionActivity, switchingSession } = useSessions()
  const durationLookup = useToolDurationLookup(currentSessionId)
  const checkpointTurns = useMemo(() => checkpoints.map(c => c.turn_index), [checkpoints])
  const rewind = useMemo(() => {
    return (msgIndex: number) => {
      const { turnIndex, rewindable } = rewindInfoFor(messages, msgIndex, checkpointTurns)
      return rewindable ? turnIndex : null
    }
  }, [messages, checkpointTurns])
  // B1 §4-7: true regenerate — only the LAST assistant message carries the
  // button, and only when the rewind+resend pipeline can actually run.
  const lastAssistantIndex = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i]?.role === 'assistant') return i
    }
    return -1
  }, [messages])
  const regenerateInfo = useMemo(
    () => (lastAssistantIndex >= 0 ? regenerateInfoFor(messages, lastAssistantIndex, checkpointTurns) : null),
    [messages, lastAssistantIndex, checkpointTurns],
  )
  const { error } = useCatalog()
  const t = useT()
  const shouldVirtualize = messages.length > VIRTUALIZE_THRESHOLD
  const messageKeys = useStableMessageKeys(messages)
  // P2-17: one run-level status region for the whole flow — the list and
  // the streaming log no longer announce every content change themselves.
  const streamActive = isQuerying || !!streamingText || !!thinkingText || activeToolCalls.length > 0

  // X-2: surface a "scroll to latest" FAB whenever the user is scrolled
  // away from the bottom. Cheap: one passive scroll listener, no re-render
  // unless the boolean actually flips.
  const [showScrollFab, setShowScrollFab] = useState(false)
  useEffect(() => {
    const el = scrollParentRef.current
    if (!el) return
    const update = () => {
      const dist = el.scrollHeight - el.clientHeight - el.scrollTop
      setShowScrollFab(dist > SCROLL_FROM_BOTTOM_THRESHOLD_PX)
    }
    update()
    el.addEventListener('scroll', update, { passive: true })
    return () => el.removeEventListener('scroll', update)
  }, [scrollParentRef, messages.length])
  const scrollToBottom = useCallback(() => {
    const el = scrollParentRef.current
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
  }, [scrollParentRef])

  // B1 §4-7: shared bubble props — the regenerate payload rides only on the
  // last assistant message; the edit affordance rides on every user message.
  const bubbleProps = (index: number) => ({
    onViewDiff: setDiffPath,
    onViewDiffMulti: setDiffPaths,
    rewindTurnIndex: rewind(index),
    onRewind: rewindSession,
    durationLookup,
    onEditMessage,
    searchFlash: searchFlashIndex === index,
    regenerate: index === lastAssistantIndex ? regenerateInfo : undefined,
  })

  return (
    <div ref={scrollParentRef} className="relative flex-1 overflow-y-auto px-xl pt-lg pb-md">
      <StreamStatusRegion active={streamActive} />
      {messages.length === 0 && !streamingText && <ComposerWelcome />}

      {messages.length > 0 && shouldVirtualize && (
        <div
          style={{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }}
          aria-label={t('chat.history.aria')}
        >
          {virtualizer.getVirtualItems().map(vItem => {
            const msg = messages[vItem.index]
            return (
              <div
                key={messageKeys[vItem.index]}
                data-index={vItem.index}
                data-message-index={vItem.index}
                ref={virtualizer.measureElement}
                className="pb-lg"
                style={{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${vItem.start}px)` }}
              >
                <MessageBubble message={msg} messageIndex={vItem.index} {...bubbleProps(vItem.index)} />
              </div>
            )
          })}
        </div>
      )}

      {messages.length > 0 && !shouldVirtualize && (
        <div aria-label={t('chat.history.aria')}>
          {messages.map((msg, i) => (
            <div key={messageKeys[i]} data-message-index={i} className="pb-lg">
              <MessageBubble message={msg} messageIndex={i} {...bubbleProps(i)} />
            </div>
          ))}
        </div>
      )}

      {/* Streaming response */}
      {(streamingText || thinkingText || activeToolCalls.length > 0) && (
        <StreamingResponse
          streamingText={streamingText}
          thinkingText={thinkingText}
          activeToolCalls={activeToolCalls}
          onViewDiff={setDiffPath}
        />
      )}

      {/* Batch C3 (ZCode「已工作 3 分 34 秒」): wall-clock status pill pinned
          to the flow bottom while a run is live — time awareness without
          expanding tool cards. */}
      {isQuerying && <RunStatusLine startedAt={currentSessionId ? sessionActivity[currentSessionId]?.startedAt ?? null : null} activeTool={currentSessionId ? sessionActivity[currentSessionId]?.activeTool ?? null : null} />}

      {error && (
        <Banner
          variant="card"
          tone="error"
          className="mx-auto max-w-md text-error font-label-md"
        >
          <span className="material-symbols-outlined icon-md text-error">error</span>
          <span className="flex-1 text-center">{error}</span>
          <ComposerRetryButton />
        </Banner>
      )}

      <div ref={messagesEndRef} />

      {showScrollFab && messages.length > 0 && (
        <Button
          type="button"
          variant="outline"
          aria-label={t('chat.scrollToLatest.aria')}
          title={t('chat.scrollToLatest.aria')}
          onClick={scrollToBottom}
          className="sticky bottom-md left-full -translate-x-full ml-sm w-10 h-10 rounded-full bg-surface-container-lowest/95 backdrop-blur-md border border-outline-variant/30 shadow-lg hover:bg-primary-container hover:border-primary/40 text-on-surface hover:text-primary transition-all flex items-center justify-center"
        >
          <span className="material-symbols-outlined icon-md" aria-hidden="true">arrow_downward</span>
        </Button>
      )}

      {/* B1 P2-3: session-swap skeleton — shown only while a switch IPC is in
          flight (AppContext never sets the flag for same-session remounts),
          so opening a session reads as instant-and-loading instead of stale. */}
      {switchingSession && (
        <div
          data-testid="session-switch-overlay"
          aria-busy="true"
          role="status"
          className="absolute inset-0 z-raised flex items-center justify-center bg-surface-container-lowest/60 backdrop-blur-[2px]"
        >
          <div className="flex flex-col items-center gap-sm">
            <span className="material-symbols-outlined text-primary animate-spin">progress_activity</span>
            <span className="font-label-sm text-on-surface-variant">{t('chat.session.switching')}</span>
          </div>
        </div>
      )}
    </div>
  )
}

// Leaf consumers of the composer context — keeping them out of MessageArea's
// render means the message list does not re-render on every keystroke.
function ComposerWelcome() {
  const { setInput } = useComposer()
  return <WelcomeState onSelectPrompt={setInput} />
}

/**
 * Batch C3: sticky status pill for a live run —「已工作 3分34秒 · 正在 bash」.
 * The startedAt/activeTool pair comes from the same SessionActivity the
 * sidebar rail consumes; a 1s tick drives the elapsed label while mounted.
 */
export function RunStatusLine({ startedAt, activeTool }: { startedAt: number | null; activeTool: string | null }) {
  const t = useT()
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(id)
  }, [])
  const elapsed = startedAt != null ? formatWorked(Math.max(0, now - startedAt)) : null
  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="run-status-line"
      className="sticky bottom-0 mt-md mx-auto w-fit flex items-center gap-xs px-md py-xs rounded-full bg-surface-container-lowest/95 backdrop-blur-md border border-outline-variant/30 shadow-sm"
    >
      <span className="size-1.5 rounded-full bg-secondary animate-pulse shrink-0" aria-hidden="true" />
      <span className="font-label-sm text-on-surface-variant whitespace-nowrap">
        {elapsed != null && t('chat.status.worked', { time: elapsed })}
        {activeTool && t('chat.status.tool', { tool: activeTool })}
      </span>
    </div>
  )
}

/** Compact running-clock label: 42s · 3m34s · 1h12m — keeps seconds so the
 *  pill visibly ticks (unlike the sidebar's coarser elapsed badge). */
function formatWorked(ms: number): string {
  const sec = Math.max(0, Math.floor(ms / 1000))
  if (sec < 60) return `${sec}s`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}m${String(sec % 60).padStart(2, '0')}s`
  return `${Math.floor(min / 60)}h${min % 60}m`
}

// B0 P1-3: the composer is cleared on send, so a retry gated on composer
// text was a guaranteed no-op. Retry now resends the LAST USER MESSAGE of
// the conversation — the same mechanism as the budget banner's "continue
// once". With no previous user message there is nothing to resend, so the
// button hides entirely.
function ComposerRetryButton() {
  const { messages, sendMessage } = useChat()
  const t = useT()
  const lastUser = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i]?.role === 'user') return messages[i]
    }
    return null
  }, [messages])
  if (!lastUser) return null
  return (
    <Button
      variant="ghost"
      className="mt-sm text-error hover:bg-error/10 text-label-md cursor-pointer"
      onClick={() => void sendMessage(lastUser.content)}
    >
      {t('chat.error.retry')}
    </Button>
  )
}
