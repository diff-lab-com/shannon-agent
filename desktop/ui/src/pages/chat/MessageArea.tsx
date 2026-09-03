import { useT } from '@/i18n'
import type { RefObject } from 'react'
import type { Virtualizer } from '@tanstack/react-virtual'
import { Button } from '@/components/ui/button'
import WelcomeState from '@/components/WelcomeState'
import { MessageBubble } from '@/components/chat/MessageBubble'
import StreamingResponse from '@/components/chat/StreamingResponse'
import { useChat } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useComposer } from './ComposerContext'
import { useMemo } from 'react'

// Virtualization only kicks in past the threshold. Below it, the overhead
// of measuring/positioning outweighs the win from fewer DOM nodes — and
// jsdom can't provide real dimensions, so tests would render zero items.
const VIRTUALIZE_THRESHOLD = 30

interface MessageAreaProps {
  scrollParentRef: RefObject<HTMLDivElement | null>
  messagesEndRef: RefObject<HTMLDivElement | null>
  virtualizer: Virtualizer<HTMLDivElement, Element>
  setDiffPath: (p: string | null) => void
  setDiffPaths: (p: string[] | null) => void
}

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

  return (
    <div ref={scrollParentRef} className="flex-1 overflow-y-auto px-xl pt-lg pb-32">
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
                <MessageBubble message={msg} messageIndex={vItem.index} onViewDiff={setDiffPath} onViewDiffMulti={setDiffPaths} rewindTurnIndex={rewind(vItem.index)} onRewind={rewindSession} />
              </div>
            )
          })}
        </div>
      )}

      {messages.length > 0 && !shouldVirtualize && (
        <div role="log" aria-live="polite" aria-label={t('chat.history.aria')}>
          {messages.map((msg, i) => (
            <div key={`${msg.timestamp}-${i}`} className="pb-lg">
              <MessageBubble message={msg} messageIndex={i} onViewDiff={setDiffPath} onViewDiffMulti={setDiffPaths} rewindTurnIndex={rewind(i)} onRewind={rewindSession} />
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
        <div className="mx-auto max-w-md p-md bg-error/10 border border-error/20 rounded-xl text-center">
          <p className="text-body-sm text-error">{error}</p>
          <ComposerRetryButton />
        </div>
      )}

      <div ref={messagesEndRef} />
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
