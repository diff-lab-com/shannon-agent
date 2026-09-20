import { useT } from '@/i18n'
import { Banner } from '@/components/ui/banner'
import type { RefObject } from 'react'
import { useEffect, useMemo, useState, useCallback } from 'react'
import type { Virtualizer } from '@tanstack/react-virtual'
import { Button } from '@/components/ui/button'
import WelcomeState from '@/components/WelcomeState'
import { MessageBubble } from '@/components/chat/MessageBubble'
import StreamingResponse from '@/components/chat/StreamingResponse'
import { useChat } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useSessions } from '@/context/SessionContext'
import { useComposer } from './ComposerContext'
import * as api from '@/lib/tauri-api'

// Virtualization only kicks in past the threshold. Below it, the overhead
// of measuring/positioning outweighs the win from fewer DOM nodes — and
// jsdom can't provide real dimensions, so tests would render zero items.
const VIRTUALIZE_THRESHOLD = 30

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
}: MessageAreaProps) {
  const { messages, streamingText, thinkingText, activeToolCalls, checkpoints, rewindSession } = useChat()
  const { currentSessionId } = useSessions()
  const durationLookup = useToolDurationLookup(currentSessionId)
  const checkpointTurns = useMemo(() => checkpoints.map(c => c.turn_index), [checkpoints])
  const rewind = useMemo(() => {
    return (msgIndex: number) => {
      const { turnIndex, rewindable } = rewindInfoFor(messages, msgIndex, checkpointTurns)
      return rewindable ? turnIndex : null
    }
  }, [messages, checkpointTurns])
  const { error } = useCatalog()
  const t = useT()
  const shouldVirtualize = messages.length > VIRTUALIZE_THRESHOLD

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

  return (
    <div ref={scrollParentRef} className="flex-1 overflow-y-auto px-xl pt-lg pb-md">
      {messages.length === 0 && !streamingText && <ComposerWelcome />}

      {messages.length > 0 && shouldVirtualize && (
        <div
          style={{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }}
          role="log"
          aria-live="polite"
          aria-label={t('chat.history.aria')}
        >
          {virtualizer.getVirtualItems().map(vItem => {
            const msg = messages[vItem.index]
            return (
              <div
                key={`${msg.timestamp}-${vItem.index}`}
                data-index={vItem.index}
                ref={virtualizer.measureElement}
                className="pb-lg"
                style={{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${vItem.start}px)` }}
              >
                <MessageBubble message={msg} messageIndex={vItem.index} onViewDiff={setDiffPath} onViewDiffMulti={setDiffPaths} rewindTurnIndex={rewind(vItem.index)} onRewind={rewindSession} durationLookup={durationLookup} />
              </div>
            )
          })}
        </div>
      )}

      {messages.length > 0 && !shouldVirtualize && (
        <div role="log" aria-live="polite" aria-label={t('chat.history.aria')}>
          {messages.map((msg, i) => (
            <div key={`${msg.timestamp}-${i}`} className="pb-lg">
              <MessageBubble message={msg} messageIndex={i} onViewDiff={setDiffPath} onViewDiffMulti={setDiffPaths} rewindTurnIndex={rewind(i)} onRewind={rewindSession} durationLookup={durationLookup} />
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
    </div>
  )
}

// Leaf consumers of the composer context — keeping them out of MessageArea's
// render means the message list does not re-render on every keystroke.
function ComposerWelcome() {
  const { setInput } = useComposer()
  return <WelcomeState onSelectPrompt={setInput} />
}

function ComposerRetryButton() {
  const { input, handleSend } = useComposer()
  const t = useT()
  return (
    <Button
      variant="ghost"
      className="mt-sm text-error hover:bg-error/10 text-label-md cursor-pointer"
      onClick={() => { if (input.trim()) handleSend() }}
    >
      {t('chat.error.retry')}
    </Button>
  )
}
